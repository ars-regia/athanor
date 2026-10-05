# Athanor languages and input

Status: **revision 1 draft, 2026-10-05: the maintainer's decisions applied; text not yet reviewed.** It specifies how the Athanor session handles languages, regions, keyboard layouts and input methods: how our crates and applications are translated, which languages and locale data the image ships, where the system and user settings are stored, how the shell's clock and calendar follow the region, how keyboard layouts are configured and switched on cosmic-comp, which input method framework the session runs and how it is confined, what the greeter shows before login, and which fonts cover non-Latin scripts. The Settings application's pages and the first-run screens are not designed here. This document defines the preferences, where they are stored, and the interface those two programs call.

## 1. Context

- **What binds this document.**
  - `doc_shell.md`: of COSMIC only cosmic-comp stays, and `athanor-compositor-client` is the only crate that may know COSMIC (SH2, SH3, SH4). Internationalisation is a requirement of every surface (`doc_shell.md:43`). The test matrix renders English, German and a right-to-left pseudo-locale, with `LC_ALL` set per case and `TZ=UTC` (SH13, `doc_shell.md:177-184`). "All strings go through gettext from the first commit" (`doc_shell.md:184`).
  - `doc_shell_standard.md`: no text is truncated in German, and the layout mirrors under a right-to-left locale (ST7).
  - `doc_bar.md`: the input-source module shows the active layout and switches between the configured ones through the compositor client (`doc_bar.md:64`). The clock follows the locale and COSMIC's `military_time` (`doc_bar.md:65`). Right-to-left mirroring is designed in `doc_bar.md:136`. Behind `wp_security_context_v1` a client loses, among other globals, the input-method and virtual-keyboard globals (`doc_bar.md:9`).
  - `doc_visual_language.md` (revision 1): the system font is Adwaita Sans 11, with Adwaita Mono for monospace. The shell names no family (VL, `doc_visual_language.md:82`).
  - `doc_software.md`, SW14: gettext from the first commit, Italian and English shipped, mirrored layout under right-to-left text (`doc_software.md:196`).
- **The register** (`shell-features.md`, v1, frozen 2026-10-04) holds these entries in this area:

  | Entry         | Feature                                  | Status in the register                                     |
  | ------------- | ---------------------------------------- | ---------------------------------------------------------- |
  | F-bar-02      | clock follows the locale                 | `have` (`clock.rs:30`)                                     |
  | F-bar-03      | clock options                            | `partial`                                                  |
  | F-bar-13      | keyboard-layout indicator with switching | `have` (`ui/input.rs`)                                     |
  | F-osd-11      | OSD on layout change                     | `missing`                                                  |
  | F-lock-05     | keyboard layout on the lock screen       | `have` via cosmic-greeter                                  |
  | F-settings-15 | language, region, date and time          | `have` via cosmic-settings                                 |
  | F-greeter-08  | date and time on the greeter             | `have`                                                     |
  | F-greeter-09  | keyboard-layout indicator on the greeter | `partial`: label only (`ui.rs:90`), switching not verified |
  | F-greeter-15  | on-screen keyboard                       | `missing`                                                  |
  | F-launcher-15 | emoji picker                             | listed                                                     |

  F-lock-05 and F-settings-15 are counted as `have` through two COSMIC programs that leave (`doc_shell.md`, SH3, stages 4 and 6), so in the end state they are `missing` until the lock and Settings specifications replace them.

- **How our crates are translated today** (repository at `visual-language-spec`, commit c0ad0e90, read on 2026-10-05).
  - `system/athanor-i18n` is a pure-Rust reader of gettext `.mo` catalogs with no dependencies. Its documentation gives three reasons for not using glibc's gettext. The greeter handles the password, so it must not use C FFI or set a global locale or text domain. glibc's gettext silently falls back to English when a locale is missing. And the reader must not depend on a toolkit (SH4).
  - It finds the language from the first non-empty value of `LC_ALL`, `LC_MESSAGES` and `LANG`, and otherwise from `LANG=` in `/etc/locale.conf`. It does **not** read the `LANGUAGE` priority list, which glibc's gettext honours.
  - It knows four plural rules as a table and treats `ar`, `he`, `fa` and `ur` as right-to-left.
  - `ATHANOR_I18N_CATALOG` overrides the catalog for tests.
  - Seven crates depend on it: `athanor-apps`, `athanor-preview`, the bar, the dock, `athanor-greeter-ui`, the launcher and the layout chooser.
  - The bar, dock, greeter UI, launcher and layout chooser each have a `po/` directory with `update.sh`, `POTFILES.in`, a `.pot`, `en.po` and `it.po`. `update.sh` runs `xgettext --language=Rust` with the crate's keywords, then `msgmerge`.
  - Each RPM spec compiles the catalogs with `msgfmt --check` into `/usr/share/locale/<lang>/LC_MESSAGES/<domain>.mo`, marked `%lang`.
  - The test rig carries German catalogs and builds a right-to-left pseudo-catalog from each `.pot` (`forge/test/shell/locale/`, `make_pseudo_rtl.py`).
  - **One crate does not follow the pattern.** `athanor-update-notify` keeps its strings in a hard-coded two-language table (`forge/specs/athanor-update/athanor-update-notify-1.0.0/src/text.rs`). A comment there says it should move to `athanor-i18n`.
- **Keyboard layouts today**, checked on 2026-10-05 on the maintainer's desktop (Athanor image, cosmic-comp `1.8.0-1.fc43.athanor1`; upstream sources at tag `epoch-1.8.0`).
  - **cosmic-comp's keyboard configuration.**
    - cosmic-comp reads its keyboard configuration from the cosmic-config key `com.system76.CosmicComp/v1/xkb_config`. The fields are `rules`, `model`, `layout`, `variant`, `options`, `repeat_delay` and `repeat_rate`, all empty by default (`cosmic-comp-config/src/lib.rs:167-200`).
    - It reloads the key live on every seat (`src/config/mod.rs:799-855`).
    - An empty field reaches libxkbcommon as an empty string. libxkbcommon then reads `XKB_DEFAULT_RULES`, `XKB_DEFAULT_MODEL`, `XKB_DEFAULT_LAYOUT`, `XKB_DEFAULT_VARIANT` and `XKB_DEFAULT_OPTIONS`, and otherwise uses its built-in defaults (libxkbcommon `xkbcommon.h`, 1.11.0).
    - cosmic-comp does not talk to systemd-localed: its binary contains neither `org.freedesktop.locale1` nor `X11Layout`.
    - The image ships no system default for `xkb_config`: `/usr/share/cosmic/com.system76.CosmicComp/v1/` holds only `input_touchpad`.
  - **cosmic-settings-daemon 1.8.0 fills the gap** (`src/locale.rs`, tag `epoch-1.8.0`).
    - At start, if the user has no `xkb_config`, it copies the layout from localed into it. Otherwise it pushes the user's `xkb_config` to localed, and keeps doing so on every change.
    - It ships a polkit rule that grants `org.freedesktop.locale1.set-keyboard` without authentication to local, active members of `wheel` or `sudo` (`data/polkit-1/rules.d/cosmic-settings-daemon.rules`).
    - cosmic-settings ships a second rule, `/usr/share/polkit-1/rules.d/cosmic-settings.rules`, that grants `org.freedesktop.locale1.set-locale` and `set-keyboard` (and three hostname and modem actions) the same way. localed's own default for both actions is `auth_admin_keep` (`/usr/share/polkit-1/actions/org.freedesktop.locale1.policy`, read 2026-10-05).
    - It also serves `InputSourceSwitch`, bound to Super+Space in COSMIC's default shortcuts (`/usr/share/cosmic/com.system76.CosmicSettings.Shortcuts/v1/defaults:102`, `system_actions:11`).
    - `doc_shell.md` (row 8 of the replacement table, `doc_shell.md:69`) has cosmic-settings-daemon leave in stage 8, so all three behaviours must have a replacement by then.
  - **The live configuration on this machine.** The user's `xkb_config` is `layout:"us", variant:"intl"`. `/etc/vconsole.conf` holds `KEYMAP=us-intl`, `XKBLAYOUT=us` and `XKBVARIANT=intl`. `/etc/locale.conf` holds `LANG=it_IT.utf8`.
  - **What the shell does with layouts today.**
    - The bar reads the layout names from the keymap and switches the active group through `zcosmic_keyboard_layout_manager_v1` (`athanor-compositor-client`, `connection.rs:214-283`; `athanor-bar`, `ui/input.rs`, `keyboard.rs`).
    - The greeter shows a read-only label (`athanor-greeter-ui`, `ui.rs:88-122`).
    - `athanor-greeter-session` exports only `LANG`, `LANGUAGE`, `LC_MESSAGES` and `LC_TIME` from `/etc/locale.conf` and no XKB variable. The greeter cosmic-comp's own `~/.config/cosmic/com.system76.CosmicComp/v1/` under `/var/lib/greetd` is empty. So the greeter's layout follows neither `/etc/vconsole.conf` nor localed (inferred, spike S4).
    - The greeter's client binds the compositor's main socket (`/usr/libexec/athanor-greeter-client`), so it sees the keyboard-layout global.
- **Input methods on cosmic-comp 1.8.0** (`src/state.rs:708-714`, tag `epoch-1.8.0`).
  - **The globals.**
    - `zwp_input_method_manager_v2`, `zwp_virtual_keyboard_manager_v1` and `zcosmic_keyboard_layout_manager_v1` are offered only to clients that are not behind a security context (`client_not_sandboxed`).
    - `zwp_text_input_manager_v3` is offered to every client.
    - The binary carries no `zwp_text_input_v1`.
    - A seat holds one input method at a time: whichever client binds first takes the slot (reported in pop-os/cosmic-osk#44, opened 2026-09-27, checked 2026-10-05).
  - **What the image has.**
    - Installed: IBus 1.5.33 with its GTK 3 and GTK 4 modules, `ibus-setup`, and the anthy, libpinyin, hangul, chewing, m17n and typing-booster engines, all from `base-atomic:43`.
    - In the Fedora 43 repository but not installed: `ibus-wayland` 1.5.33 (the input-method-v2 front end, `/usr/libexec/ibus-wayland`), `ibus-panel` (the candidate window), and fcitx5 5.1.21 with `fcitx5-gtk4` and the Chinese, anthy, hangul and chewing addons.
  - **How GTK 4.20.4 picks its input-method module.** With `GTK_IM_MODULE` unset, GTK 4 uses the highest-priority module that matches the display.
    - The built-in `wayland` module has priority 100 and requires `zwp_text_input_manager_v3` (`gtk/gtkimcontextwayland.c:102-105`, `gtk/gtkimmodule.c:111-123`, tag 4.20.4).
    - IBus's GTK 4 module has priority 50 (`client/gtk4/ibusim.c`, tag 1.5.33).
    - So GTK 4 applications on cosmic-comp use text-input-v3 even with `ibus-gtk4` installed.
  - **Upstream state, checked 2026-10-05.**
    - IBus added input-method-v2 in 1.5.32, started by `ibus start --type wayland`, and dropped custom pre-edit colours in that mode (https://desktopi18n.wordpress.com/2025/01/13/ibus-1-5-32-plan/).
    - cosmic-comp needs IBus 1.5.32 or later, or fcitx5 5.1.12 or later (pop-os/cosmic-session#185, opened 2026-02-04, open).
    - fcitx5's own guidance for input-method-v2 compositors (https://fcitx-im.org/wiki/Using_Fcitx_5_on_Wayland):
      - leave `GTK_IM_MODULE` unset;
      - set `XMODIFIERS` for XWayland;
      - set `QT_IM_MODULES="wayland;fcitx"` for Qt 6.8.2 and later;
      - pass `--enable-wayland-ime --wayland-text-input-version=3` to Chromium.
      - Electron applications support only text-input-v1, which cosmic-comp does not offer.
  - **Open defects upstream, relevant here.**
    - pop-os/cosmic-comp#2702 (opened 2026-08-06, open): the compositor does not revoke an input method's keyboard grab when the session locks, so keys typed on the lock screen, passwords included, reach the input-method client.
    - pop-os/cosmic-comp#2778: menus do not open while fcitx5 is active.
    - pop-os/cosmic-comp#1968: the input-method popup is not scaled.
    - #2778 and #1968 were found by search on 2026-10-05; their pages were not read.
- **Locale data on the image** (checked on 2026-10-05, glibc 2.42-16.fc43).
  - `glibc-all-langpacks` is installed with an installed size of 237,950,035 bytes. `localedef --list-archive` lists 887 locales.
  - `system/Containerfile:252` keeps glibc-all-langpacks deliberately.
  - `forge/config/packages.json:104-105` also install `glibc-langpack-en` (5,992,542 bytes) and `glibc-langpack-it` (3,720,981 bytes). These duplicate data that all-langpacks already ships.
  - `/usr/share/locale` holds 699 language directories (335 MB), because Fedora keeps translations inside the main packages (`%_install_langs` is `all`).
  - composefs is enabled (`/usr/lib/ostree/prepare-root.conf`), so the two identical locale archives (`locale-archive` and `locale-archive.real`, 233,242,544 bytes each, same sha256) are stored once.
- **Fonts** (checked on 2026-10-05).
  - `adwaita-sans-fonts` 49.0 covers Latin, Greek, Cyrillic and Vietnamese only (`fc-query` on `AdwaitaSans-Regular.ttf`).
  - `base-atomic:43` installs `default-fonts-*` for about 70 languages. These include the Noto Sans and Serif variable families for most scripts, Noto Sans CJK, Vazirmatn for Persian, Padauk for Burmese and Noto Color Emoji.
  - `fc-match sans-serif:lang=X` resolves:

    | Language                | Font                            |
    | ----------------------- | ------------------------------- |
    | ar                      | Noto Sans Arabic                |
    | fa                      | Vazirmatn                       |
    | he                      | Noto Sans Hebrew                |
    | ja / zh-cn / zh-tw / ko | Noto Sans CJK JP / SC / TC / KR |
    | hi                      | Noto Sans Devanagari            |
    | bn                      | Noto Sans Bengali               |
    | ta                      | Noto Sans Tamil                 |
    | th                      | Noto Sans Thai                  |
    | ka                      | Noto Sans Georgian              |
    | am                      | Noto Sans Ethiopic              |
    | km                      | Noto Sans Khmer                 |
    | my                      | Padauk                          |
    | el / ru / vi            | Noto Sans                       |

  - The shell test rig installs Inter and two Noto families but not `adwaita-sans-fonts` (`forge/test/shell/Containerfile:17`).
- **Accounts and region** (checked on 2026-10-05).
  - AccountsService 23.13.9 stores a user's language as `Language` (one locale) and `Languages` (an ordered list), set through `SetLanguage` and `SetLanguages`. It has no property for formats.
  - GNOME stores the region in the GSettings key `org.gnome.system.locale region`. At login gnome-session sets `LC_NUMERIC`, `LC_TIME`, `LC_MONETARY`, `LC_PAPER`, `LC_ADDRESS`, `LC_TELEPHONE` and `LC_MEASUREMENT` to it, and leaves them unset when the region equals `LANG` (gnome-session `leader-main.c:129-158`, main at d3c610c1, 2026-09-20).
  - GTK 4.20.4 calls `setlocale(LC_ALL, "")` at initialisation (`gtk/gtkmain.c:452`). `GtkCalendar` takes the first day of the week from `nl_langinfo(_NL_TIME_FIRST_WEEKDAY)`, so it follows `LC_TIME`.
  - GNOME's wall clock looks up its time-format strings in the catalog of the `LC_TIME` locale, not of the messages locale (gnome-desktop `libgnome-desktop/gnome-wall-clock.c:288-306`, master, read 2026-10-05).
  - `org.gnome.desktop.interface clock-format` (`'24h'` or `'12h'`) exists in gsettings-desktop-schemas 49.1.
- **Where the session starts today.**
  - `athanor-session` exports `XDG_CURRENT_DESKTOP` and `XDG_DATA_DIRS`, then execs cosmic-comp. It sets no locale, XKB or input-method variable.
  - `athanor-desktop` imports only `WAYLAND_DISPLAY`, `DISPLAY`, `XDG_CURRENT_DESKTOP`, `XDG_SESSION_TYPE` and `XDG_SESSION_CLASS` into the user manager (`athanor-desktop:34`). So the bar and every other user unit run with the user manager's locale, which comes from `/etc/locale.conf`, not the user's own.
  - The greeter's `StartSession` passes only `XDG_SESSION_TYPE` and `XDG_CURRENT_DESKTOP` (`athanor-greeter-ui`, `auth.rs:268-274`).
  - `system/athanor-oobe` is an unpackaged first-run prototype. It maps language names to locales and writes `/etc/locale.conf` and `/etc/vconsole.conf` directly when `localectl` fails (`main.rs:40-95`). It is not a model for this document.

## 2. Decisions

**LN1. One translation format: gettext.**

- Every string in our crates, applications, desktop entries, AppStream metainfo, polkit `.policy` files and feature files is translated through gettext `.po` catalogs. The source language is English, and the English text is the msgid.
- Each program has its own text domain, named after its package (`athanor-bar`, `athanor-greeter-ui`, and so on). Compiled catalogs go to `/usr/share/locale/<lang>/LC_MESSAGES/<domain>.mo`.
- **Two readers, one format.**
  - Programs with no GTK type, the shell's GTK4 programs and the greeter read catalogs through `athanor-i18n`. The greeter's reasons for its own reader (no C FFI and no global locale in the password process) stand.
  - Our libadwaita applications (the file manager, the disk utility, Settings, Software) read them through glibc's gettext, bound by the `gettext-rs` crate (0.8.0, https://crates.io/crates/gettext-rs, checked 2026-10-05) with its `gettext-system` feature, so that the crate links glibc's gettext instead of building its own copy. At start each application calls `setlocale(LC_ALL, "")`, `bindtextdomain(<domain>, "/usr/share/locale")`, `bind_textdomain_codeset(<domain>, "UTF-8")` and `textdomain(<domain>)`. GtkBuilder `.ui` files and Blueprint are allowed in these applications.
  - A program uses one reader, never both.
  - glibc's gettext falls back to English silently when a catalog is missing. In the applications that fallback is caught by LN3's CI check, which requires a complete catalog for every shipped language.
- Fluent, Qt's `.ts`, JSON message files and hard-coded tables are not used. `athanor-update-notify`'s table in `text.rs` moves to `athanor-i18n` and a `po/` directory in construction step 1.

**LN2. `athanor-i18n` follows glibc's language selection.**

- The language list is built in glibc's order:
  1. `LANGUAGE`, a colon-separated priority list. It counts only when the effective `LC_MESSAGES` locale is not `C` or `POSIX`, as in glibc.
  2. Otherwise the first non-empty value of `LC_ALL`, `LC_MESSAGES` and `LANG`.
  3. Otherwise `LANG=` in `/etc/locale.conf`.
- Each entry expands to `ll_CC`, then `ll`, as it does today.
- A second lookup, `Catalog::for_category(domain, Category::Time)`, finds the catalog through the `LC_TIME` locale (`LC_ALL`, `LC_TIME`, `LANG`). It falls back to the messages catalog when no catalog exists for that locale. LN8 uses it.
- The reader stays free of C FFI and of `setlocale`. A missing catalog is logged once at warning level with the domain and the languages tried, and the program continues in English. This is the documented behaviour today, kept.
- Right-to-left detection stays a table. The table gains `ckb`, `ps`, `sd`, `ug` and `yi`.

**LN3. The translation workflow of a crate.**

- Each crate with user-visible strings has `po/` with `POTFILES.in`, `LINGUAS`, `<domain>.pot` and one `<lang>.po` per shipped language. The existing `update.sh` stays the way to regenerate them. `LINGUAS` is new: it lists the shipped languages, and the RPM spec loops over it instead of naming each language.
- Every string carries context for translators where a word is ambiguous: `tr_c`, or a `TRANSLATORS:` comment above the call, which `xgettext --add-comments=TRANSLATORS:` keeps.
- Plurals go through `tr_n`, never through a concatenated number.
- Desktop entries, metainfo and `.policy` files are templates merged at RPM build time with `msgfmt --desktop` or `msgfmt --xml`. A feature TOML file (`doc_software.md`, SW5) keeps its English name and sentence. The Software application translates them at run time in the domain the file names (`gettext_domain` key).
- **CI checks**, one script under `scripts/` called by the lint workflow:
  - each `.pot` is current: `update.sh` run in a temporary copy produces no change to the msgids;
  - each `.po` passes `msgfmt --check`;
  - no `.po` in `LINGUAS` has untranslated or fuzzy entries on a release branch.
- **Translators** contribute through pull requests on the `.po` files, reviewed like any other change. No hosted translation service has write access to the repository in the first release. The format allows one later without change.

**LN4. Languages and locale data in the image.**

- **Our catalogs** ship for Italian and English (SH13, SW14). The test rig adds German and the right-to-left pseudo-locale, which never ship.
- **glibc locales.** All of them stay through `glibc-all-langpacks`, which the base image already ships (`system/Containerfile:252`). `glibc-langpack-en` and `glibc-langpack-it` leave `forge/config/packages.json`: together they hold 9.7 MB of locale data that all-langpacks already provides.
  - No locale is generated at first boot, and `/etc` holds no locale archive, so every locale a user picks works on the immutable image without a rebuild.
- **Translations of third-party packages** stay as Fedora ships them: all languages, inside each package.
- **Spell-checking dictionaries** ship for each shipped language: `hunspell-en` is already installed, and `hunspell-it` (1,441,631 bytes) is added. Other dictionaries come from Software as described in LN15.
- **The language list offers every language with a glibc locale,** marked by what the user gets.
  - The list is built from the UTF-8 entries of `locale -a` (326 entries for about 209 languages besides `C`, glibc-common 2.42-16.fc43, counted 2026-10-05), one entry per language and country.
  - A language in the installed `LINGUAS` list (LN16) is marked "complete": every Athanor program has its catalog.
  - Any other language is marked "partial", with the sentence "Athanor's own surfaces stay in English". GNOME and third-party applications are translated as Fedora ships them (above).
  - Complete languages are listed first, then the partial ones.
  - A partial language can be chosen and works: our programs fall back to English as LN2 says.

**LN5. The system locale belongs to systemd-localed.**

- The system locale (`/etc/locale.conf`) and the system keyboard (`/etc/vconsole.conf`, `X11Layout` and related properties) are written only through `org.freedesktop.locale1`: `SetLocale`, `SetVConsoleKeyboard` and `SetX11Keyboard` with `convert` set to true.
- localed's own polkit actions (`org.freedesktop.locale1.set-locale` and `set-keyboard`) authorise the calls, and both require an administrator's authorisation, kept for the polkit session (`auth_admin_keep`, localed's own default in `/usr/share/polkit-1/actions/org.freedesktop.locale1.policy`).
- **The rule that keeps that default.** Two COSMIC rules grant both actions to local, active members of `wheel` or `sudo` without authentication (section 1). Athanor ships `40-athanor-locale1.rules` to override them:
  - **File:** `/usr/share/polkit-1/rules.d/40-athanor-locale1.rules`, in the package `athanor-base-config`, which already installs `/usr/share/polkit-1/rules.d/*` (`forge/specs/athanor-base-config/athanor-base-config.spec:76`).
  - **Content:** one `polkit.addRule` that returns `polkit.Result.AUTH_ADMIN_KEEP` when `action.id` is `org.freedesktop.locale1.set-locale` or `org.freedesktop.locale1.set-keyboard`, for every subject, and returns nothing for any other action.
  - **Order:** polkit processes the rules files of `/etc/polkit-1/rules.d` and `/usr/share/polkit-1/rules.d` in lexical order of their basename and stops at the first rule that returns a result (polkit(8), polkit 126-6.fc43.2, read 2026-10-05). `40-athanor-locale1.rules` sorts after `10-athanor-wheel-admin.rules`, which makes `wheel` the administrators, and before `50-default.rules`, `cosmic-settings-daemon.rules` and `cosmic-settings.rules`.
  - **Effect:** a member of `wheel` changes the system locale or keyboard after typing their own password. cosmic-settings-daemon's push of a user's layout to localed (LN9) is refused without a prompt, because it calls `SetX11Keyboard` with `interactive` false (`src/locale.rs:70-77`, tag `epoch-1.8.0`), and the refusal is only logged by the daemon.
  - **Lifetime:** the file stays after the COSMIC rules leave with cosmic-settings and cosmic-settings-daemon, so that no later package can grant these actions silently. A program that needs them without a password (first run, LN16) gets a rule of its own, scoped to its own subject, in a file that sorts before `40-athanor-locale1.rules`; each such rule is a polkit change the maintainer approves.
  - It is not a change to `system/athanor-bus-api/src/polkit.rs`. It is built and judged as its own construction step (LN19, step 3).
- No Athanor program writes these files directly.
- The system locale governs the greeter, system services, the console, and any user who has not chosen a language of their own.

**LN6. The user's language belongs to AccountsService.**

- A user's language is AccountsService's `Languages` (an ordered list), with `Language` set to its first entry for programs that read only that property.
- The Settings application writes them through `SetLanguages`, under the polkit action AccountsService defines for a user's own data. Which action applies, and whether it asks for a password, is unverified (spike S8).
- **At login**, `athanor-session` reads the user's own `Languages` from AccountsService before it execs cosmic-comp:
  - it exports `LANG` from the first entry;
  - it exports `LANGUAGE` from the list when the list has more than one entry;
  - when the property is empty it exports nothing, and the system locale applies.
- Values are checked against `[A-Za-z0-9_.@-]`, as `athanor-greeter-session` already checks them. A value that fails is logged and ignored.
- The greeter passes no language in `StartSession`, and `auth.rs` does not change. The session reads its own user's setting itself, so nothing the greeter process holds decides the session's environment.
- `athanor-desktop` adds `LANG`, `LANGUAGE` and the `LC_*` variables of LN7 to its `import-environment` call, so the user units (the bar, the dock, `athanor-shelld`, the launcher) run in the user's locale.
- A change of language takes effect at the next login. The Settings page says so, and offers to log out.

**LN7. Region formats.**

- The region (dates, times, numbers, currency, paper size, measurement units, first day of the week) is a locale stored in GSettings `org.gnome.system.locale region`, empty by default. Reusing GNOME's key keeps GNOME applications and `gnome-control-center`'s semantics.
- Before reading the language (LN6) and the region, `athanor-session` runs `athanor-first-run apply-handoff` (`doc_first_run.md` FR15), bounded to 2 s, whose failure never blocks the login.
- At login, `athanor-session` applies it as gnome-session does (`leader-main.c:129-158`):
  - when the key is non-empty and differs from `LANG`, it exports `LC_NUMERIC`, `LC_TIME`, `LC_MONETARY`, `LC_PAPER`, `LC_ADDRESS`, `LC_TELEPHONE` and `LC_MEASUREMENT` set to the region;
  - otherwise it exports none of them.
- It reads the key with `gsettings get`.
- The region list offers every locale in the archive (887 on 2026-10-05), shown with its formatted example date and number.
- A change of region takes effect at the next login, as for the language.
- The time zone is not a region setting: it belongs to `org.freedesktop.timedate1` and to the Settings and first-run specifications.

**LN8. The shell's clock and calendar follow the region.**

- **Date and time patterns** stay translatable msgids in the bar's domain, as `ui/clock.rs` has them today. The bar looks them up in the catalog of the `LC_TIME` locale (LN2, `Category::Time`), as GNOME's wall clock does. A user with English messages and an Italian region then reads the day before the month. When no catalog exists for the region's language, the patterns come from the messages catalog. Day and month names always come from `LC_TIME` through GLib.
- **12 or 24 hours.** The setting is GSettings `org.gnome.desktop.interface clock-format` (`'24h'` or `'12h'`). It replaces COSMIC's `military_time`, which leaves with cosmic-settings.
  - When the user has never set the key (no user value in dconf), the bar follows the region: a 24-hour clock when the `LC_TIME` locale's `%p` is empty, as `clock.rs` decides today.
  - `athanor_compositor_client::clock` is removed once the bar reads the new key.
- **The first day of the week.** The calendar in the bar's popover and in the notification panel takes the first day of the week from `LC_TIME` through `GtkCalendar`. No setting of our own overrides it.
- **Week numbers** follow `org.gnome.desktop.calendar show-weekdate`.

**LN9. Keyboard layouts: the store, the writer, the switch.**

- **The store** is cosmic-comp's own `xkb_config`, which cosmic-comp reloads live. It holds the user's layouts, variants, model, options and repeat settings. No synchronisation daemon mirrors it. `org.gnome.desktop.input-sources` is not used and stays empty; input-method engines stay in IBus's own settings (LN11). If cosmic-comp is ever replaced, the store moves with the compositor client.
- **The only writer** is `athanor-compositor-client`. It gains a `keyboard` module:
  - **Reading.** `read() -> XkbLayouts` returns the ordered layouts with their variants, the model, the options and the repeat settings.
  - **Writing.** `write(&XkbLayouts)` validates every layout and variant against `/usr/share/X11/xkb/rules/evdev.xml` and writes atomically, with the same `athanor_layout::atomic::write_atomically` used by `theme.rs` and `shortcuts.rs`.
  - **Names.** `available()` lists the layouts and variants from `evdev.xml`, each with its translated description from xkeyboard-config's `xkeyboard-config` text domain.
  - At most four layouts. XKB holds four groups, and cosmic-comp joins the comma-separated lists into one keymap.
  - The Settings application and first run call this module and never write the file themselves.
- **The seed.** When a user has no `xkb_config`, `athanor-shelld` copies localed's `X11Layout`, `X11Variant`, `X11Model` and `X11Options` into it at session start, through the module. This is the behaviour of cosmic-settings-daemon 1.8.0, kept.
- **No push back.** A user's layout change never changes localed. The system keyboard changes only through LN5.
  - Until cosmic-settings-daemon leaves in stage 8, its push of every `xkb_config` change to localed stays in the daemon but fails: `40-athanor-locale1.rules` (LN5) requires an administrator, and the daemon's call is non-interactive, so localed refuses it and the daemon logs the refusal.
  - Its copy from localed into an empty `xkb_config` at start does the same as the seed above and is harmless.
- **Switching.**
  - The bar's input-source module switches through `zcosmic_keyboard_layout_manager_v1`, as today (F-bar-13).
  - Super+Space, cosmic-comp's `InputSourceSwitch` system action, moves to the next layout through `athanor-shelld`:
    - the session daemon gains a `NextInputSource` method on its private interface, admitted from cosmic-comp's action process;
    - the `system_actions` entry in `/usr/share/athanor/cosmic-defaults` points to it.
    - This replaces cosmic-settings-daemon's `InputSourceSwitch`.
  - XKB group options such as `grp:alt_shift_toggle` keep working, because libxkbcommon handles them inside cosmic-comp.
- **The `system_actions` file.**
  - **Path and owner:** `/usr/share/athanor/cosmic-defaults/cosmic/com.system76.CosmicSettings.Shortcuts/v1/system_actions`, shipped by `athanor-calmo`, which owns the directory. No other package owns the file.
  - **Contents:** COSMIC 1.8.0's full map with Athanor's entries replacing those of the same name: `InputSourceSwitch` (LN9), `ScreenReader` (`doc_accessibility.md` AX3), `Screenshot` (`doc_portal.md` PT8) and `WorkspaceOverview` (`doc_overview.md` OV17), each added in the step of the draft that names it. A file earlier in `XDG_DATA_DIRS` replaces COSMIC's whole map, not one entry, so the file keeps every other entry of the system file (`doc_launcher.md` LA8).
  - **Users with an LA8 copy** receive a later system entry by a re-merge in the compositor client's `set_system_action_once`, or, if cosmic-comp merges the user map over the system map per entry, by nothing. Which applies is checked on cosmic-comp 1.8.0 (spike S10) and recorded here.
- **What the bar shows.**
  - The module appears only with two or more layouts (`keyboard.rs`, `shown`).
  - It shows the short name of the active layout ("IT", "US"), with the variant in the tooltip and the accessible name ("Input source: English (US, intl.)").
  - When an input method is active (LN12), the module shows the input method's state instead, as LN12 says.
- **The OSD.** A layout change by shortcut shows the new layout's name in the on-screen display (F-osd-11).
  - The interface is a `KeyboardLayout` event of the compositor client's model (`model.rs` already has `KeyboardLayouts`). The OSD program subscribes to it.
  - `doc_osd.md` owns the surface. This document owns the event and its text.
  - A change made from the bar's popover shows no OSD, because the popover already shows it.
- **The lock screen** shows the active layout and lets the user switch with the same compositor-client calls. `doc_lock_and_prompts.md` owns the surface.

**LN10. The greeter's language and layout.**

- **Language.** The greeter runs in the system locale. This is already the case: `athanor-greeter-session` exports `LANG`, `LANGUAGE`, `LC_MESSAGES` and `LC_TIME` from `/etc/locale.conf`. It shows no per-user language before login.
- **Layout.** `athanor-greeter-session` exports `XKB_DEFAULT_LAYOUT`, `XKB_DEFAULT_VARIANT`, `XKB_DEFAULT_MODEL` and `XKB_DEFAULT_OPTIONS` from the `XKBLAYOUT`, `XKBVARIANT`, `XKBMODEL` and `XKBOPTIONS` lines of `/etc/vconsole.conf`, which localed keeps in step with `SetX11Keyboard`.
  - It parses them without sourcing the file and with the same character check it uses for the locale variables. Commas are also allowed, because the lists are comma-separated.
  - With no `xkb_config` in the greeter user's configuration, cosmic-comp then builds the system layout (spike S4).
  - No Athanor program writes `xkb_config` for the `greetd` user.
- **Switching.** When the system has two or more layouts, the greeter's layout label becomes a switcher. It is a menu button that lists the layouts and calls `set_keyboard_group` through `athanor-compositor-client`, which the greeter client can use because it binds the main socket.
  - The current read-only label (`ui.rs:88-122`) is replaced.
  - The password field keeps its focus after a switch.
- **No input method in the greeter.** Passwords are typed with XKB layouts only. The greeter's compositor runs no input-method client, so cosmic-comp#2702 cannot occur there.

**LN11. Input methods: framework and enablement.**

- **The framework is IBus.** It is already in the base image with six engines, it is what GNOME applications and Fedora expect, and `ibus-portal` serves Flatpak applications that use IBus's own module. Spike S1 confirms the choice. If IBus fails S1 on a criterion that fcitx5 5.1.21 meets, the choice is reversed and this rule and LN12 are rewritten for fcitx5.
- **Off by default.**
  - A user enables an input method by adding an input-method engine in Settings. The engine list is IBus's `org.freedesktop.ibus.general preload-engines`, written through GSettings, and IBus owns it.
  - The Settings application lists only the engines installed in the image. On 2026-10-05: anthy (Japanese), libpinyin (Chinese, simplified), chewing (Chinese, traditional), hangul (Korean), m17n (many Indic and other scripts) and typing-booster.
- **The daemon.**
  - When the engine list is not empty, `athanor-ime.service`, a user unit started by `athanor-session.target`, runs `ibus-daemon` in Wayland mode (`ibus start --type wayland`, which runs `ibus-wayland`) with the candidate panel. When the list becomes empty, the unit stops.
  - `ibus-wayland` and `ibus-panel` join the image.
- **The environment.** When the unit is enabled, `athanor-session` exports `XMODIFIERS=@im=ibus` (XWayland), `QT_IM_MODULES="wayland;ibus"` and `QT_IM_MODULE=ibus`, as gnome-session does when ibus-daemon is present (`leader-main.c`, d3c610c1).
  - It never sets `GTK_IM_MODULE`. GTK 3 and 4 use text-input-v3 through their `wayland` module, which outranks IBus's module (section 1).
- **Chromium and Electron.**
  - Chromium-based browsers from Flatpak receive `--enable-wayland-ime --wayland-text-input-version=3` through their Flatpak override. Software applies it when the input method is enabled.
  - Electron applications that support only text-input-v1 get no input method on cosmic-comp, and are listed as a known limit.
- **The bar.** With the input method active, the bar's input-source module shows the active engine's short label, and its popover lists the engines next to the layouts. Super+Space (LN9) cycles through the layouts, then the engines, in one list that `athanor-shelld` composes from the two stores. IBus's own trigger key is set to empty, so that the key is not bound twice (spike S6).
  - The bar and `athanor-shelld` talk to IBus over its D-Bus address through a new crate, `athanor-ibus`, with no GTK type. `athanor-compositor-client` stays the only crate that knows COSMIC.
- **The emoji picker** (F-launcher-15) is not IBus's `ibus-ui-emojier`. `doc_launcher.md` owns it.

**LN12. Input methods: confinement and the lock screen.**

- **Why the daemon cannot be confined like an application.** An input-method client must bind `zwp_input_method_manager_v2`, which cosmic-comp withholds from clients behind a security context. So the daemon connects to the main socket, like the greeter client and `athanor-shelld`. This is a named exception to the confinement of shell surfaces, and it is confined by systemd instead:
  - **filesystem:** `ProtectSystem=strict`, with write access only to IBus's own cache and configuration directories under the user's home (`ReadWritePaths=`);
  - **network:** `PrivateNetwork=yes`, because no engine shipped in the image needs the network;
  - **memory and system calls:** `MemoryDenyWriteExecute=yes` where the engines allow it (spike S7), and `SystemCallFilter=@system-service`;
  - **devices:** `NoNewPrivileges=yes` and `PrivateDevices=yes`.
  - An engine added later that needs the network (a cloud engine) is excluded.
- **The lock screen.** cosmic-comp#2702 sends keys typed on the lock screen to the input method that holds the keyboard grab.
  - Until spike S2 shows that our cosmic-comp build hands the keyboard to the lock surface while an input method is active, the input method cannot be enabled in a release image.
  - If the defect holds on 1.8.0, our cosmic-comp package carries a patch that drops the input method's grab while the session is locked and restores it on unlock. We already carry a patch (the layer-surface focus patch in `1.8.0-1.fc43.athanor1`), and this one is proposed upstream.
  - The lock specification's acceptance includes this case (section 3).
- **One slot per seat.** cosmic-comp gives a seat's single input-method slot to the first client that binds it (cosmic-osk#44). `athanor-osk`, the on-screen keyboard of `doc_accessibility.md` (its decision 5), runs on the input-method and virtual-keyboard protocols, so it competes with IBus for that slot. Both documents state this rule:
  - while an IBus engine is enabled, IBus holds the slot, and `athanor-osk` sends its keys through `zwp_virtual_keyboard_v1` only, so they reach the active engine like keys from a hardware keyboard;
  - with no engine enabled, `athanor-osk` may hold the slot;
  - `athanor-osk` never binds the slot while an engine is enabled, and releases it before `athanor-ime.service` starts. How `athanor-osk` learns of the change is `doc_accessibility.md`'s.
  - Spike S9 verifies the hand-over and that virtual-keyboard keys reach the engine.

**LN13. Fonts for every script.**

- **The interface font** is the system font, Adwaita Sans 11 (`doc_visual_language.md:82`). No Athanor program names a family.
- **Scripts Adwaita Sans lacks** are drawn by fontconfig's fallback to the fonts `base-atomic:43` installs (section 1). The image adds no font and removes none of the `default-fonts-*` packages.
- **The test rig** installs `adwaita-sans-fonts` and `adwaita-mono-fonts`, so the goldens use the font the image uses. It also adds `google-noto-sans-hebrew-fonts` and `google-noto-sans-cjk-vf-fonts`, so a right-to-left case and a CJK fallback case can be rendered.
- **A new font case** in the rig renders the bar's clock and a launcher result with an Arabic, a Japanese and a Hindi application name. Besides the golden, the case asserts that Pango's `pango_layout_get_unknown_glyphs_count` is zero for each string.

**LN14. Flatpak applications follow the user's languages.**

- Flatpak installs translations of runtimes and applications as `.Locale` extensions, limited by the `languages` and `extra-languages` keys of the installation (https://docs.flatpak.org/en/latest/flatpak-command-reference.html, checked 2026-10-05).
- The image sets nothing for the system installation, so flatpak uses its default. Whether that default reads AccountsService's languages of every user, or only the current locale, is unverified (spike S3).
- If it reads only the current locale, the Software application adds the user's languages to `extra-languages` when the user changes them. This needs a privilege that `doc_software.md` already defines for system installs.

**LN15. Preferences and storage.** The Settings application owns the pages, and first run owns its screens. Both read and write only these:

| Preference                        | Store                                                    | Writer interface                                         | Takes effect                                                             |
| --------------------------------- | -------------------------------------------------------- | -------------------------------------------------------- | ------------------------------------------------------------------------ |
| System language                   | `/etc/locale.conf` via localed                           | `org.freedesktop.locale1.SetLocale`                      | next boot of the greeter; users without their own language at next login |
| System keyboard                   | `/etc/vconsole.conf` via localed                         | `org.freedesktop.locale1.SetX11Keyboard` (convert true)  | greeter at next start, console                                           |
| User languages                    | AccountsService `Languages`, `Language`                  | `org.freedesktop.Accounts.User.SetLanguages`             | next login                                                               |
| Region                            | GSettings `org.gnome.system.locale region`               | GSettings                                                | next login                                                               |
| Clock format                      | GSettings `org.gnome.desktop.interface clock-format`     | GSettings                                                | immediately                                                              |
| Week numbers                      | GSettings `org.gnome.desktop.calendar show-weekdate`     | GSettings                                                | immediately                                                              |
| Keyboard layouts, options, repeat | cosmic-comp `xkb_config`                                 | `athanor_compositor_client::keyboard::write`             | immediately                                                              |
| Input-method engines              | GSettings `org.freedesktop.ibus.general preload-engines` | GSettings, then `athanor-ime.service` started or stopped | immediately                                                              |
| Spelling dictionaries             | packages (Flatpak or image)                              | Software                                                 | after install                                                            |

- The XDG Settings portal serves `clock-format` among the `org.gnome.desktop.interface` keys, so that Flatpak clocks agree with the bar. `doc_portal.md` owns that list.

**LN16. The interface first run needs.** First run (`doc_first_run.md`) sets language, region and keyboard in this order, with the calls of LN15, and nothing else:

1. **System language and keyboard,** before any user exists: `SetLocale` and `SetX11Keyboard` with convert true. First run runs as the user that `doc_first_run.md` defines and holds localed's polkit actions through a rule of its own that sorts before `40-athanor-locale1.rules` (LN5). Choosing a language switches first run's own text at once: it reloads its catalog with the new `LANGUAGE`, which `athanor-i18n` supports after LN2.
2. **The first user's** `Languages` (the same as the system's unless changed), region (empty unless changed), and `xkb_config`, which `athanor-shelld` seeds at the first login (LN9), so first run does not write it.
3. **The time zone,** through `org.freedesktop.timedate1`, which first run owns.

- The list of languages and the list of layouts come from functions first run and Settings share:
  - `athanor_i18n::shipped_languages()` reads the `LINGUAS` list installed at `/usr/share/athanor/i18n/LINGUAS`;
  - `athanor_i18n::offered_languages()` returns the list of LN4: each UTF-8 locale of `locale -a` with its "complete" or "partial" mark from `shipped_languages()`, complete ones first;
  - `athanor_compositor_client::keyboard::available()` (LN9).
- The suggested layout for a chosen language is the layout of the language's main country in `evdev.xml` ("it" for `it_IT`), and the user confirms it.

**LN17. Tests.**

- **Logic, on every change in CI:**
  - `athanor-i18n` unit tests for the `LANGUAGE` list, `C` and `POSIX` handling, the `LC_TIME` lookup and its fallback, the plural table and right-to-left detection;
  - `offered_languages()` against a fixture `locale -a` list and a fixture `LINGUAS` (LN4);
  - the keyboard module's validation against a fixture `evdev.xml`;
  - the seed and no-push logic of LN9 against a fake localed;
  - the session script's export of `LANG`, `LANGUAGE` and `LC_*` from fixtures, with the character check;
  - the CI checks of LN3.
- **Surfaces, in the rig:**
  - the matrix of SH13 (English, German, the right-to-left pseudo-locale), rendered with Adwaita Sans;
  - the clock's cases {`LANG=en_US`, `LC_TIME=it_IT`} and {`LANG=it_IT`, `LC_TIME=en_US`} in 12 and 24 hours;
  - the calendar starting on Monday for `it_IT` and on Sunday for `en_US`;
  - the font case of LN13.
- **On the reference laptop,** the acceptance of section 5.

**LN18. Exclusions.** These are outside the first release, each recorded as `excluded` in the register with its reason:

- text-input-v1 and therefore input methods in Electron applications that lack text-input-v3;
- custom pre-edit colours (IBus input-method-v2 does not support them);
- per-window input sources;
- a per-user language on the greeter before login;
- automatic switching of layout by application;
- online (cloud) input engines;
- a hosted translation service (LN3);
- translations of our crates beyond Italian and English;
- a regional choice of the first day of the week separate from the region;
- an on-screen keyboard (owned by `doc_accessibility.md`).

**LN19. Construction.** Each step merges on its own and is installed on the reference laptop and judged there by the maintainer before it merges. The logic's tests run on every change.

1. **The translation foundation, with no visible change.**
   - `athanor-i18n` gains `LANGUAGE`, the `LC_TIME` lookup and the wider right-to-left table.
   - Each crate gains `LINGUAS`, and the RPM specs loop over it.
   - The CI checks of LN3.
   - `athanor-update-notify` moves to `athanor-i18n`.
   - `glibc-langpack-en` and `glibc-langpack-it` leave `packages.json`, and `hunspell-it` joins it.
   - The rig installs Adwaita Sans and Mono, and the goldens are regenerated once.
2. **The user's language and region reach the session.**
   - `athanor-session` exports `LANG`, `LANGUAGE` and `LC_*` from AccountsService and the region key.
   - `athanor-desktop` imports them into the user manager.
   - The bar's clock reads its patterns through `LC_TIME` and its 12- or 24-hour choice from `clock-format`.
   - `athanor_compositor_client::clock` is removed.
3. **The locale1 polkit rule, on its own.** A polkit change, built and judged by itself.
   - `40-athanor-locale1.rules` in `athanor-base-config` (LN5).
   - Judged on the reference laptop as a member of `wheel`: `pkcheck --action-id org.freedesktop.locale1.set-keyboard --process $$` reports that authentication is required, and the same for `set-locale`; a layout change in cosmic-settings leaves `localectl status` unchanged, and cosmic-settings-daemon's journal shows the refused push.
4. **Keyboard layouts without cosmic-settings-daemon.**
   - The compositor client's `keyboard` module.
   - The seed and Super+Space in `athanor-shelld`.
   - The `system_actions` entry.
   - The `KeyboardLayout` event for the OSD.
   - The greeter's XKB variables and its switcher.
   - After this step cosmic-settings-daemon no longer switches layouts. Its push to localed already fails since step 3.
5. **Input methods, behind spikes S1, S2, S6, S7 and S9.**
   - The cosmic-comp patch for the lock screen if S2 needs it.
   - `ibus-wayland` and `ibus-panel` in the image.
   - `athanor-ime.service` and its sandbox.
   - The environment of LN11.
   - `athanor-ibus` and the bar's engine display.
   - The Chromium override in Software.
6. **The font case and the rest of the rig cases of LN17,** then the acceptance of section 5 signed by the maintainer.

## 3. Changes to other documents

Applied with the approval of this document. Line numbers into code are those of the tree as merged with iso-v0 (99a68285), checked on 2026-10-05; those into `doc_software.md` are of revision 3 as merged (PR #117), and those into the other desktop specifications are of their text on 2026-10-05.

- **`doc_shell.md`.**
  - SH13's "Italian and English are the shipped locales" (`doc_shell.md:177`) stays. Its rig gains the clock cases of LN17 and the font case of LN13.
  - "All strings go through gettext from the first commit" (`doc_shell.md:184`) gains: "read through `athanor-i18n` in the shell and the greeter, and through `gettext-rs` and glibc's gettext in our libadwaita applications (`doc_languages.md`, LN1)".
  - The change of the interface family at `doc_shell.md:96` is `doc_visual_language.md`'s, not repeated here.
- **`doc_bar.md`.**
  - The clock row (`doc_bar.md:65`): "the system clock, formatted for the locale, in 12 or 24 hours as COSMIC's clock setting says (`military_time`), else as the locale's time format" becomes "the system clock, in 12 or 24 hours as `org.gnome.desktop.interface clock-format` says, else as the region's time format; date and time patterns from the catalog of the `LC_TIME` locale (`doc_languages.md`, LN8)".
  - The input-source row (`doc_bar.md:64`) adds: "with an IBus engine enabled, the active engine's short label instead of the layout's, and the engines listed next to the layouts in the popover (LN11)".
- **`shell-features.md`.**
  - F-bar-02 (`shell-features.md:31`): the 12- or 24-hour setting is `org.gnome.desktop.interface clock-format` (LN8); the evidence moves to the bar's new clock code at LN19 step 2.
  - F-bar-03 (`shell-features.md:32`): the clock options are `clock-format` and `show-weekdate` (LN8).
  - F-bar-13 (`shell-features.md:42`): the indicator also shows and switches IBus engines (LN11).
  - F-osd-11 (`shell-features.md:296`): the interface is the compositor client's `KeyboardLayout` event (LN9).
  - F-lock-05 (`shell-features.md:314`) and F-settings-15 (`shell-features.md:362`): from `have` via cosmic-greeter and cosmic-settings to `missing` in the end state, owned by `doc_lock_and_prompts.md` and `doc_settings.md`.
  - F-greeter-09 (`shell-features.md:423`): `partial` until LN19 step 4, then `have`.
  - F-greeter-15 (`shell-features.md:429`): owned by `doc_accessibility.md` (`athanor-osk`). No input method runs in the greeter (LN10), so the slot rule of LN12 does not apply there.
  - The exclusions of LN18 are added as rows with their reasons.
- **`doc_osd.md`** (the keyboard-layout row at `doc_osd.md:91`): subscribes to the compositor client's `KeyboardLayout` event and shows the layout's short and full name (LN9).
- **`doc_lock_and_prompts.md`**:
  - shows and switches the layout through the compositor client (LN9);
  - its acceptance (section 5, `doc_lock_and_prompts.md:228`) adds the case of cosmic-comp#2702: "with an IBus engine enabled and holding the keyboard grab, the lock screen receives every key, the password included, and the input method none" (LN12). Its spike L2 (`doc_lock_and_prompts.md:220`) and S2 here run the same check;
  - the lock screen, like the greeter, offers no input method.
- **`doc_accessibility.md`** (AX12 at `doc_accessibility.md:133-138`, decision 5 at `doc_accessibility.md:252`): states the slot rule of LN12. `athanor-osk` and IBus compete for the single input-method slot per seat (cosmic-osk#44); while an engine is enabled, IBus holds the slot and `athanor-osk` types through `zwp_virtual_keyboard_v1` only. The accessibility spike on the on-screen keyboard and S9 here cover it together.
- **`doc_portal.md`**: the Settings backend serves `org.gnome.desktop.interface clock-format` (LN15).
- **`doc_settings.md`**:
  - the Language and Region, Keyboard and Input-method pages use only the stores and interfaces of LN15, and say that a language or region change takes effect at the next login;
  - the language list is `athanor_i18n::offered_languages()`, with the "complete" and "partial" marks of LN4;
  - Settings reads its own catalogs through `gettext-rs` (LN1).
- **`doc_first_run.md`**:
  - uses the interface of LN16;
  - the polkit rule that lets first run call `SetLocale` and `SetX11Keyboard` sorts before `40-athanor-locale1.rules` and names first run's own subject (LN5). It is a polkit change the maintainer approves.
- **`doc_software.md`** (revision 3):
  - SW5's feature file (`doc_software.md:103`) gains the `gettext_domain` key;
  - SW14's "gettext from the first commit" (`doc_software.md:196`) gains "through `gettext-rs` with glibc's gettext (`doc_languages.md`, LN1)";
  - Software applies the Chromium input-method override (LN11);
  - Software extends Flatpak's `extra-languages` if spike S3 requires it (LN14).
- **`forge/specs/athanor-base-config/`:** adds `SOURCES/usr/share/polkit-1/rules.d/40-athanor-locale1.rules`, installed by the existing `/usr/share/polkit-1/rules.d/*` line (`athanor-base-config.spec:76`) (LN5, LN19 step 3).
- **`forge/test/shell/Containerfile:17`:** adds `adwaita-sans-fonts`, `adwaita-mono-fonts`, `google-noto-sans-hebrew-fonts` and `google-noto-sans-cjk-vf-fonts` (LN13).
- **`forge/config/packages.json`:** removes `glibc-langpack-en` and `glibc-langpack-it` (`packages.json:104-105`), and adds `hunspell-it` (LN4). `ibus-wayland` and `ibus-panel` are added at LN19 step 5.
- Amendments received, from the maintainer's rulings of 2026-10-05: LN7 gains the first-run hand-off step (`doc_first_run.md` FR15); the `system_actions` file of LN9 is defined here as the single owner of that file.

## 4. Open doubts

Each is unverified until its spike runs. Spikes run on the reference laptop and in the development VM, never on the maintainer's desktop.

- **S1. Which input-method framework works on cosmic-comp 1.8.0.** IBus 1.5.33 with `ibus-wayland` and `ibus-panel`, and fcitx5 5.1.21, each with:
  - GTK 4 (GNOME Text Editor), GTK 3, Qt 6, Firefox, Chromium with text-input-v3, and an XWayland program;
  - Japanese (anthy or mozc), Chinese (libpinyin) and Korean (hangul).
  - Observed for each:
    - the candidate popup is placed at the cursor and scaled at 1.5 (cosmic-comp#1968);
    - menus open while the input method runs (cosmic-comp#2778);
    - IBus's panel draws its candidates through `zwp_input_popup_surface_v2` rather than as a top-level window;
    - the framework exposes a StatusNotifierItem the bar's tray can show.
  - IBus (LN11, decision 5) is confirmed by this spike, or reversed as LN11 says.
- **S2. The lock screen with an input method** (cosmic-comp#2702 on 1.8.0). Enable an engine, focus a text field, lock with `loginctl lock-session`, type. Pass when cosmic-greeter's lock surface (today) receives every key and the engine's log shows none. If it fails, the patch of LN12 is written and the spike repeated on the patched build.
- **S3. Flatpak's default languages for the system installation.** On a fresh install with the system in `en_US` and a user in `it_IT`, install a GNOME runtime and check whether the `it` subset of its `.Locale` extension is installed.
- **S4. The greeter's layout from `XKB_DEFAULT_*`.** Set the system keyboard to `it` through localed, restart greetd, type in the greeter's user field. Inferred from libxkbcommon's documentation and cosmic-comp's empty defaults, not observed.
- **S5. A new user's first layout without cosmic-settings-daemon.** With cosmic-settings-daemon masked and the seed of LN9 in place, create a user and log in. Pass when the layout is localed's, not libxkbcommon's default "us".
- **S6. Super+Space and IBus's trigger.** IBus's default trigger is `<Super>space` (`org.freedesktop.ibus.general.hotkey triggers`, read 2026-10-05), the same key as cosmic-comp's `InputSourceSwitch`. Check which one receives the key with both bound, and that an empty IBus trigger plus our one list (LN11) switches engines and layouts in order.
- **S7. The input method's sandbox.** `PrivateNetwork=` and `MemoryDenyWriteExecute=` in a user unit, with each shipped engine. The typing-booster engine is Python, and m17n may load code.
- **S8. The authorisation for `SetLanguages` on one's own account.** Which polkit action applies on AccountsService 23.13.9, and whether it asks for a password for an active local user.
- **S9. The input-method slot shared with `athanor-osk`** (cosmic-osk#44), run together with the on-screen keyboard spike of `doc_accessibility.md`. With `athanor-osk` holding the slot, enable an IBus engine: pass when `athanor-osk` releases the slot before `athanor-ime.service` binds it and IBus then holds it. With IBus holding the slot, type on `athanor-osk`: pass when its keys, sent through `zwp_virtual_keyboard_v1`, reach the active engine and produce candidates as hardware keys do. Whether cosmic-comp routes virtual-keyboard keys through the input method's keyboard grab is unverified until this spike.
- **S10. How cosmic-comp 1.8.0 resolves `system_actions`.** On the reference laptop, with the file of LN9 installed and a per-user copy written under LA8 that lacks one entry of the system file: pass when that entry's action still runs, which shows that cosmic-comp merges the user map over the system map per entry. Otherwise the user copy replaces the whole map, and LN9's re-merge in `set_system_action_once` is built. It decides whether LN19 step 4 includes that re-merge.

## 5. Acceptance

Observable criteria. "CI" runs on every change. "Reference laptop" means athanor-ref, judged by the maintainer on screen.

1. **CI:** the tests of LN17 pass. No `.po` of a shipped language has untranslated or fuzzy entries. Every `.pot` is current.
2. **CI:** `athanor-update-notify` builds its catalogs from `po/`, and its `text.rs` table is gone.
3. **CI, rig:** the SH13 matrix in English, German and the right-to-left pseudo-locale renders with Adwaita Sans, with no truncated German string and a mirrored layout.
4. **CI, rig:** the clock shows "lun 5 ott 16:40" for `LANG=en_US.UTF-8`, `LC_TIME=it_IT.UTF-8` in 24 hours, and "Mon Oct 5 4:40 PM" for `LANG=it_IT.UTF-8`, `LC_TIME=en_US.UTF-8` in 12 hours. The exact strings are fixed by the Italian and English catalogs at step 2.
5. **CI, rig:** the calendar's first column is Monday for `it_IT` and Sunday for `en_US`.
6. **CI, rig:** Arabic, Japanese and Hindi names render with no missing-glyph box.
7. **Reference laptop:**
   - Settings language set to Italian and region to United States, then log out and in.
   - The bar, dock, launcher and a GNOME application are in Italian.
   - The clock shows 12-hour time with the month before the day.
   - The calendar starts on Sunday.
   - `systemctl --user show-environment` shows `LANG=it_IT.UTF-8` and `LC_TIME=en_US.UTF-8`.
8. **Reference laptop:**
   - Two layouts configured. Super+Space switches them, and the OSD names the new layout.
   - The bar's indicator follows, and its popover switches back.
   - The layout chosen holds across logout and login.
   - `localectl status` is unchanged by these switches.
9. **Reference laptop:**
   - The system keyboard set to `it` through localed, then greetd restarted.
   - The greeter types Italian characters, and its switcher changes to the second system layout.
   - A user created afterwards starts with `it`.
10. **Reference laptop, after step 5:**
    - Japanese and Chinese input in GNOME Text Editor, Firefox and Chromium (Flatpak), with candidates at the cursor at scale 1.5.
    - Locking the screen with the input method active, the lock screen accepts the password and the engine receives no key (journal of `athanor-ime.service` at debug level).
11. **Reference laptop:** `systemd-analyze --user security athanor-ime.service` reports the properties of LN12. With the input method enabled, `ss -p` inside the unit's network namespace shows no socket other than loopback.
12. **Reference laptop:** `rpm -q glibc-langpack-en glibc-langpack-it` reports "not installed". `locale -a` lists 887 locales or more. `LANG=ja_JP.UTF-8 date` prints Japanese.
13. **Reference laptop, after step 3:** as a member of `wheel`, `pkcheck --action-id org.freedesktop.locale1.set-locale --process $$` and the same for `set-keyboard` report that authentication is required. A layout change made in the session leaves `localectl status` unchanged.
14. **CI:** `athanor_i18n::offered_languages()`, run on a fixture `locale -a` list and a fixture `LINGUAS` with `en` and `it`, returns English and Italian first, marked "complete", and every other UTF-8 locale after them, marked "partial".

## 6. Decisions taken

Taken by the maintainer on 2026-10-05. Each followed the recommendation of revision 0.

1. **How our libadwaita applications read translations.** Choice: `gettext-rs` (glibc's gettext) in the applications, `athanor-i18n` kept for the shell and the greeter (LN1). Reason: the format stays one, the reader follows the toolkit where GtkBuilder forces it, and the greeter keeps its reasons for its own reader.
2. **How translators contribute.** Choice: pull requests on the `.po` files for release 1 (LN3, LN18). Reason: only two languages ship, and the format allows a hosted service later without change.
3. **Which languages the language list offers.** Choice: every language with a glibc locale, each marked "complete" or "partial", complete ones first (LN4, LN16). Reason: it serves everyone (the rule of 2026-09-14) and says honestly what each user gets.
4. **Where a user's keyboard layouts are stored.** Choice: cosmic-comp's `xkb_config`, written only through `athanor-compositor-client` (LN9). Reason: it is the store cosmic-comp reads live, its only writer is the crate already allowed to know COSMIC, and nothing has to be kept in sync.
5. **Which input-method framework.** Choice: IBus, confirmed or reversed by spike S1 (LN11). Reason: it is already in the base image with six engines and is what GNOME applications and Fedora expect.
6. **cosmic-settings-daemon's keyboard behaviour until it leaves.** Choice: `40-athanor-locale1.rules` returns `AUTH_ADMIN_KEEP` for localed's two actions, built as its own construction step (LN5, LN9, LN19 step 3). Reason: it ends a silent system-wide write from a user setting without waiting for stage 8, and the system keyboard stays changeable with an administrator's authorisation.

**Open doubts left by the decisions.**

- Decision 5 stands until spike S1 runs: if IBus fails S1 on a criterion fcitx5 meets, LN11 and LN12 are rewritten for fcitx5.
- The input-method slot shared with `athanor-osk` (cross-document ruling of 2026-10-05): the rule of LN12 holds until spike S9, run with the accessibility spike, shows the hand-over works.
