# Athanor shell: direction, layout model, stages

Status: **revision 5, 2026-09-25, awaiting the maintainer's approval.** It carries the maintainer's correction of that day (section 1): of COSMIC only cosmic-comp stays, and our own bar and dock come next. It rewrites SH1, SH2, SH3 and section 3, and changes SH4, SH6, SH7, SH9, SH12, SH13 and sections 4 to 8 where they assumed cosmic-panel. Revision 3 was approved on 2026-09-18; revision 4, approved by the maintainer on 2026-09-24, added outputs taller than wide to SH7, SH10, SH13 and sections 4, 5 and 7. For revision 3 the maintainer took the decisions and delegated the validation. Revision 1 failed two independent reviews, an adversarial one and a security one: the direction held, the update and trust part did not. Revision 2 took their findings, both reviewers then found it approvable with changes, and revision 3 takes those changes. The four reports are `.superpowers/shell-spec-review.md`, `shell-spec-security-review.md` and their `-r2` successors.

Stage 1 is implemented: package 1a is merged (PR #55), packages 1b-system and 1c are delivered by PRs #64 and #63, and the gate of section 7 has not run yet. Package 1b-shield is not built; package 1d, the seal in the greeter, takes its place. Stage 2 starts with spike P4, and its bar and dock get a specification of their own, `doc_bar.md`, written after P4. Section 5 lists what is still unverified.

The three spikes of section 3 ran on 2026-09-18 (`.superpowers/spike-p1-applet.md`, `spike-p2-gtk-bump.md`, `spike-p3-headless.md`). Their results are written into SH4, SH5, SH12 and SH13 below; none overturned a decision. Spike P4 has not run yet.

The document replaces `doc_shell_ui.md` and amends `doc_platform_experience.md`, section 3. Section 6 lists the changes other documents take.

## 1. Context

Since 2026-09-11 the desktop is Fedora's COSMIC 1.8 on cosmic-comp: panel, dock, launcher, settings, notifications. `athanor-shell-rs` survived only as the greeter (on 2026-09-18; since stage 1a the greeter is `athanor-greeter-ui`). The maintainer wants a proprietary shell back, evolved rather than restored, because the graphical environment is what an average user judges the system by.

What the analyses found in the old shell:

- It was a niri satellite. Workspaces, dock and overview are dead on cosmic-comp.
- Its visual identity never rendered: 50 GTK4 CSS parse errors, `backdrop-filter` is web-only, 34 of 185 classes defined.
- It had facades: a constant RAM reading, a wallpaper palette computed from compressed bytes.
- It had no accessibility roles, no i18n, no tray, no multi-monitor, `GDK_SCALE=1`, and `#![allow(clippy::all, warnings)]`.
- About 2,000 lines are worth keeping: the greeter with `sys/auth.rs`, the notification server, the D-Bus proxies, the spring integrator.

What the platform gives us today:

- cosmic-comp 1.8 advertises every protocol a shell needs. Privilege depends on which Wayland socket a client holds; the sandbox engine string `com.system76.CosmicPanel` is not authenticated.
- libcosmic is not applicable to our surfaces: not on crates.io, no releases, a hard fork of iced, and its text inputs emit no accessibility nodes (pop-os/libcosmic#1429).
- cosmic-panel is a nested compositor, so an applet is a plain xdg toplevel in any toolkit. It starts the applet ids its configuration lists and resolves each id through the XDG data path, where `~/.local/share` precedes `/usr/share`. A process running as the user can therefore shadow an applet's desktop file, and the user can remove any applet in COSMIC Settings.
- `org.kde.StatusNotifierWatcher` is owned by `cosmic-applet-status-area`. Removing cosmic-panel removes the tray.
- The client verifies no image signature. `/etc/containers/policy.json` is `insecureAcceptAnything`, the booted reference is `ostree-unverified-registry:`, and the pipeline signs keyless (`forge/scripts/sign_attest.sh`), which `containers-policy.json` can match only by `subjectEmail`; a GitHub Actions identity has none.
- Background updates do not run: the preset enables `bootc-fetch-apply.timer`, which does not exist, and the override of `bootc-fetch-apply-updates.service` calls `bootc upgrade --stage`, a flag bootc 1.16 does not have.

**The maintainer's correction, 2026-09-25.** Revision 4 kept COSMIC's panel, dock and headless daemons by choice, and put a panel of our own late in a list of later stages. The maintainer's goal, in their words translated: "our goal is to have the compositor as the only dependency on COSMIC, with GTK4, in a modular way, so that if we have to change it in the future it is easy to do." The staged path holds, and so does the rule for replacing a surface. What changes is the end state (SH1, SH3), how the compositor is contained (SH2, SH4) and the order: our bar and dock come next, before any other surface, and the session shield is born inside our bar instead of as an applet of cosmic-panel.

## 2. Decisions

**SH1. Staged path to one dependency.** COSMIC's surfaces and daemons are replaced one stage at a time, until cosmic-comp is the only COSMIC component the session runs.

- **Stage 1** (implemented): a real design system, the update experience and the trust state, on top of COSMIC. Section 3 records it.
- **Stage 2:** our bar and our dock, on a compositor client that holds every dependency on COSMIC (SH2). The session shield is born inside our bar.
- **Later stages,** in this order unless a stage's own spec argues otherwise: 3, the launcher and the application library; 4, the password prompts, session lock and polkit agent together, because both reuse the greeter's authentication code and both are the trusted path; 5, the on-screen display, unless it already left with its agent in stage 4; 6, Settings; 7, the workspace overview; 8, the headless daemons: settings daemon, idle, wallpaper. Visible surfaces come first.
- **The rule for replacing a surface:** ours is usable by an average user and better than COSMIC's at the moment of the switch. Until then ours is enabled by hand and the image keeps COSMIC's. Tray, fractional scaling, screen-reader roles and i18n are requirements of every surface, not extras.
- **No facades.** A control that does nothing, or a reading with no source, is a defect.

**SH2. cosmic-comp stays; the dependency has an exit.** We do not write a compositor.

- A Rust compositor is 2.6–3.3 MB of Rust on top of Smithay's 4.4 MB, and its cost is the hardware matrix, not the first version. A compositor of our own on Smithay would still depend on the maintainer of cosmic-comp, through the library.
- cosmic-comp is GPL-3 and Smithay is MIT. A fork is always possible, and we only run cosmic-comp as a separate process.
- **Containment in one crate.** `athanor-compositor-client` is the only crate of the shell that may depend on COSMIC at run time: the `cosmic-*` protocols, `cosmic-client-toolkit`, the `cosmic-config` keys, read by path as `athanor-style` does today because `cosmic-config` is not on crates.io and the workspace allows no git source, any configuration of cosmic-comp the shell writes, such as shortcuts or workspace behaviour, and the reading of `CosmicTheme` for the mode and the accent (SH5), which moves there from `athanor-style`. It is a library that every surface links, not a daemon: each surface keeps its own Wayland connection, and no new IPC surface appears.
- **Its interface is ours.** It exposes our own types: an open window, a workspace, an output with its shape (SH7), the actions activate, minimise and close, and a stream of events. No COSMIC type crosses it. It is not a trait with one implementation: changing compositor means rewriting the inside of this crate, and an abstraction arrives with a second compositor, if one ever does.
- **Standard protocols are used anywhere:** `ext-workspace-v1`, `ext-foreign-toplevel-list-v1`, `ext-image-copy-capture-v1`, `ext-session-lock-v1`, `ext-idle-notify-v1`, and `wlr-layer-shell` through the shim (SH4). They survive a change of compositor, and where one covers a need the compositor client uses it too.
- **Exceptions, by path.** `forge/tools/calmo-cosmic-theme` generates, by hand when the tokens or the libcosmic revision change, the committed default `CosmicTheme` that COSMIC applications read (SH3, SH5); the build fails when that output is stale. Until the switch of stage 2, the bridge of package 1c: `forge/specs/athanor-layout-translator`, and `system/athanor-layout/src/cosmic.rs` and `apply.rs`, which leave with it (SH7).
- **The boundary is checked.** `scripts/verify.py` fails when a crate other than these names a dependency `libcosmic` or `cosmic-*`, or when Rust source outside them names a `com.system76` configuration. The check covers Rust only; the Python token generator under `system/athanor-style/calmo/` also writes COSMIC defaults, which are content for COSMIC applications. `system/athanor-style/src/cosmic_theme.rs` fails the check today: package 2a moves it into the compositor client in the change that adds the check. The boundary is a failing check, not a review comment.
- **Security features only a compositor can give** (authenticated privileged clients, a trusted path for credential prompts, compositor-drawn trust decorations) are proposed upstream first. If upstream declines, they become a small isolated patch set built in the forge and rebased per release.
- **A fork is reconsidered only when** System76 abandons or relicenses cosmic-comp, or a security requirement we cannot drop is declined and the patch set stops being maintainable, or the project has more maintainers. A fork starts from cosmic-comp, never from zero.

**SH3. Of COSMIC only cosmic-comp stays.** The shell owns pixels and plumbing; cosmic-comp owns the screen and the input.

| Component | Role | Leaves in stage | Replaced by |
|---|---|---|---|
| cosmic-panel, cosmic-applets | panel, dock, the tray's `org.kde.StatusNotifierWatcher` (in `cosmic-applet-status-area`), network, Bluetooth, audio, battery, power, input source | 2, at the switch | our bar (2b) and our dock (2c) |
| cosmic-notifications | notification daemon, today a child of the panel's wrapper on an inherited socket pair | 2, at the switch | our notification server, with the bar |
| cosmic-launcher, cosmic-app-library | launcher, application library | 3, or 2 if P4 finds they need the panel | our launcher |
| cosmic-greeter as locker | session lock | 4 | our lock, on the greeter's authentication code |
| cosmic-osd | on-screen display and the session's only polkit agent | 4, or 5 if it can run without its agent | our polkit agent (4) and on-screen display (5) |
| cosmic-settings, cosmic-randr | Settings application, output configuration | 6 | our Settings; `athanor-settings-rs` is not revived, only mined |
| cosmic-workspaces | overview | 7 | our overview |
| cosmic-settings-daemon, cosmic-idle, cosmic-bg | configuration bus and media keys; `org.freedesktop.ScreenSaver` and idle policy; wallpaper | 8 | our daemons |

The table is the end state and its order, not a calendar. A component leaves only when its replacement passes SH1's rule; until then the image keeps it.

COSMIC applications (`cosmic-files`, `cosmic-term`, `cosmic-edit`, `cosmic-store`) are content, not dependencies. They read `CosmicTheme`, so the design system writes it (SH5).

**SH4. GTK4, with the shim kept replaceable.**

- Our surfaces use GTK4 through plain `gtk4-rs`. AT-SPI, IME and RTL work today.
- **One crate per program** (maintainer's decision, 2026-09-19). Every surface is its own binary in its own crate, with only the dependencies, the Landlock rules and the review surface it needs: the greeter first, because it handles the password. Shared code lives in a library crate only when two programs really use it: `athanor-style` for tokens and CSS, `athanor-trust-state` for the state file and the badge, `athanor-compositor-client` for everything the shell knows about COSMIC (SH2). `athanor-shell-rs`, the single binary that held the greeter until stage 1a, beside 13,000 lines of surfaces this document calls dead, is not bumped: it leaves the workspace frozen at GTK 0.7 and is mined for the code worth keeping until it is empty, then deleted. `athanor-dock`, its path dependency, is treated the same way; the dock of stage 2 is a new crate.
- **relm4 is upgraded, not dropped** (spike P2, on `iso-v0` at cfe61ecf). With gtk4 0.11.4, gtk4-layer-shell 0.8.1 and relm4 0.11.0 the shell crate has 54 errors, none caused by relm4: 25 old-syntax `glib::clone!`, 20 `Option` wraps, 4 removed channel APIs that need `async-channel`, 5 others. That is a day to a day and a half, and it is not spent: the greeter uses no relm4, needs one line, and moves to its own crate.
- **The GTK crates move together or leave the workspace.** `gtk4-sys` declares `links = "gtk-4"`, so no crate can be pinned apart. On 2026-09-18 (spike P2) nine crates touched gtk4, two of them only through `athanor-style`; two shipped, `athanor-shell-rs` and `athanor-recovery`, and `athanor-dock` is a path dependency of the first. The plan of 1a proposes which unshipped crates leave `[workspace.members]` before the bump; the maintainer decides, because it retires code.
- `gtk4-layer-shell` is a one-maintainer symbol-interposition shim that GTK does not support. Therefore: one process per surface, all logic in toolkit-agnostic crates, and the shim called from one place per surface. If the shim fails, only the bar and the dock move to a direct `wayland-rs` client. Qt is not a fallback.
- **The shim degrades silently,** to a normal window with a title bar, when it is not loaded before `libwayland-client` (spike P3). Our Rust binary links it first today, by the accident of link-flag order and not by contract (spike P2). Therefore the package's `%check` asserts the `DT_NEEDED` order, and every layer-shell surface asserts at start that it is a layer surface and exits with an error when it is not.
- **Always-on surfaces use GTK's Cairo renderer:** the bar and the dock. With the default renderer an icon-sized applet maps 216–320 MB; with Cairo it is 41 MB PSS, beside 33 MB for a stock applet (spike P1). P4 measures a bar that is a top-level layer surface.
- `#![allow(clippy::all, warnings)]` is removed from any crate a stage touches.

**SH5. Identity: "Calmo".** Light and neutral by default, with a dark variant of equal standing.

- **Tokens are the single source.** One tokens file generates the GTK4 CSS of our surfaces and the default `CosmicTheme`, light and dark. CI parses the generated CSS with GTK's own parser; a parse warning fails the build.
- **Calmo is the default, not a migration.** COSMIC's RPMs own the files under `/usr/share/cosmic/`, so our defaults cannot be packaged at those paths. The plan of 1a picks the mechanism: an overlay directory ahead of `/usr/share` in `XDG_DATA_DIRS` if cosmic-config honours it, otherwise a build step after the COSMIC RPMs, checked by `verify.py shipped`. A user whose `~/.config/cosmic` already holds a theme keeps it.
- **Factory accent:** indigo, hue 231 and saturation 62 % in the HSL tokens (`#2e44c2` on light, `#8898f7` on dark). The user changes the accent; trust colours (verified, attention, blocked) are fixed and never derived from the accent.
- **Stage 1 has one accent control, COSMIC's.** GTK inherits nothing from COSMIC: on the maintainer's desktop COSMIC is dark while GTK's colour scheme says `default`, and a GTK surface comes up light (spike P1). Our surfaces therefore read `CosmicTheme` themselves, for the mode and for the accent, and compute the on-accent text colour against WCAG AA at run time. The greeter uses the factory accent. A curated palette arrives with a Settings surface of our own, not before.
- **Contrast is validated:** every text/background pair of the tokens meets WCAG AA in four variants, light and dark, each normal and high-contrast, checked in CI. Our surfaces follow COSMIC's high-contrast flag. Every animation has a disabled path that follows the reduced-motion setting.
- **The identity lives in form, not in colour,** because the colour is the user's. Two signatures carry it:
  - **The mark is the seal.** The Athanor mark is reserved for the trust shield (SH12) and appears nowhere else in the shell; the launcher uses a neutral glyph.
  - **The hearth wallpaper.** The default wallpaper is a set of concentric discs rising from a corner. Stage 1 ships it as two images, light and dark, in the factory accent. It follows the user's accent only when the accent comes from a curated palette, because each hue is then an image built ahead of time.
- **Shipped assets:** Inter (`rsms-inter-fonts`) as the only interface family, with tabular figures for clocks, to be confirmed on the dev VM; `cosmic-icon-theme` stays in stage 1; our own symbolic icons only for the seal and its states.
- **Depth is reserved for what floats:** windows, popovers and notifications carry a marked shadow; panels and docks carry a hairline and a faint one.
- Nothing in the identity relies on an effect GTK4 cannot draw.

**SH6. The layout is a document with our own schema.**

- Keyed by output from the first version. Version 1 accepts only the wildcard key, `[output."*"]`; a named output is a later schema version.
- Versioned: `schema = <integer>`.
- Layered, lowest to highest: vendor (`/usr/share/athanor/layout/`, replaced on update) < policy (`/etc/athanor/layout/`) < user (`~/.config/athanor/layout.toml`).
- **The policy layer is not a security boundary,** and no wording in the interface says it is. It sets defaults, and a key it marks mandatory is honoured by our loader and greyed in our chooser. While cosmic-panel reads a user-writable configuration, a user can still move the panel behind our back; enforcement arrives with our own bar, at the switch of stage 2.
- The document names a preset and the knobs of SH7, nothing else. It is the serialisation of what the interface offers, never a superset. The accent is not in it: in stage 1 the accent belongs to the theme (SH5).

**SH7. Presets are data.** A preset is a style plus factory values for the knobs. Version 1 ships three presets and two knobs.

| Id | Display name | Style | Factory panel | Factory dock |
|---|---|---|---|---|
| `float` | Isola | floating rounded panel (COSMIC 1.8.0 ships an edge-to-edge one), floating centred dock as COSMIC ships it | top | visible |
| `bar` | Barra | one edge-to-edge bar holding launcher, running applications, tray and clock | bottom | no knob |
| `minimal` | Essenziale | thin edge-to-edge bar | top | none |

- **Identifiers are permanent and English; display names are translated.** A fourth preset, three separate islands with a vertical dock (`split`), needs our own bar and is not in version 1; it may come after stage 2 as a new schema version (SH8).
- **Knobs:** panel position (top, bottom); dock (visible, auto-hide, none).
- **A knob exists only where it means something.** `bar` holds the applications in the bar, so it has no dock knob and the schema rejects one. That leaves 2 × 3 layouts for `float`, 2 for `bar` and 2 × 3 for `minimal`: 14 in version 1.
- **Dock placement is derived, not chosen,** from the panel position and from the shape of each output. On an output wider than tall the dock sits on the bottom edge, and on the left edge when the panel is at the bottom. On an output taller than wide it never takes a side edge, which would spend the scarce dimension: it sits on the bottom edge, stacked above the panel when the panel is at the bottom. A vertical dock carries icons only (SH9.3).
- **Rotation matters only through the shape.** A flipped or mirrored output keeps its logical geometry and takes the layout of the unrotated one; a quarter turn (90° or 270°) makes it taller than wide. The document stays keyed by `[output."*"]`: the shape is read from each output, never stored.
- **`bar` on cosmic-panel is an icon taskbar:** no COSMIC applet renders window titles. Titles come with our bar, after a decision on who may read them, which `doc_bar.md` takes with P4's answer on what each client can see. `minimal` is subject to cosmic-panel#560 being absent on 1.8.
- Until the switch of stage 2 a translator turns the effective document into cosmic-panel configuration. It writes `entries` last, because that configuration is live and not atomic. It runs when the effective document changes and when an output is added, removed or rotated, because the dock's edge depends on each output's shape; it therefore stays resident in the session and watches the outputs, but it rewrites nothing at login unless the result differs, and it is idempotent. It writes one dock for every output while all of them share a shape, and one dock per output otherwise. cosmic-panel's configuration is not the user's layout document: SH8's promise covers `layout.toml`, not the files under `~/.config/cosmic`.
- **While our bar is enabled by hand,** cosmic-panel keeps running and draws only the dock, because it draws the panel and the dock in one process: the translator then writes only the dock entry. At the switch the translator and the cosmic-panel module of `athanor-layout` are removed. The schema, the loader and the presets stay, and our bar and dock read them directly.

**SH8. The schema is the ratchet.** Everything the schema can express is supported and tested; nothing else is expressible.

- **Reading:** the shell reads every schema version it ever shipped and migrates in memory. A preset or key removed in a later version maps to a named successor in a migration table shipped with the schema.
- **Rejecting:** an unknown key, a newer `schema`, or a malformed file rejects the whole user document. The shell then applies the nearest preset it knows and logs at error priority. A rollback to an older `/usr` therefore degrades, and a later upgrade restores.
- **Writing:** the shell writes the user document only when the user changes the layout, at the current schema. It never rewrites it on its own. When the rejected document has a newer schema, the chooser asks before saving and keeps the old file as `layout.toml.<schema>`.
- **Crash-loop protection:** five failures within ten minutes on `CLOCK_BOOTTIME`, the policy `/usr/bin/athanor-cosmic-panel` applies today, then the vendor layout. The shell is never lost.

**SH9. Invariants, never configurable.**

1. The trust shield is present whenever a panel is present, at the same place within a preset. Without a panel the trust state stays reachable from the greeter and from the update notifications. cosmic-panel carries no shield (SH12). In our bar the shield is a fixed module that no layout can remove; a process running as the user can still draw over it (SH12).
2. No third-party code runs in our surfaces: modules are a closed, in-image set. A third-party module mechanism may exist only inside a compartment with a declared capability manifest.
3. Elements that carry text are horizontal.

**SH10. The default preset is chosen once per user,** at the first session, from the smallest logical height among the connected outputs; Wayland has no primary output. Below 800 logical pixels the pick is `bar`, which spends one edge instead of two; otherwise `float`. A 1080p laptop at 150 % is 720 logical pixels and gets `bar`: that is intended. When any connected output is taller than wide the pick is `float` whatever the heights: `bar` lays the running applications and the tray along the scarce dimension and runs out of room there first, while `float` spends height, which such an output has. The pick writes only the `preset` key, and nothing at all when the policy layer names a preset. A marker under `$XDG_STATE_HOME/athanor/` records that it ran, so it is never re-evaluated. The chassis type is not used: `hostnamectl chassis` answers `vm` or nothing on too many machines.

**SH11. Updates are never forced, and never applied unconfirmed.** Stage 1b is the interim implementation of the Athanor update service of `doc_kernel_profile.md`, section 8 (D31, D36), behind the same interface, so `systemd-sysupdate` with A/B `/usr` later replaces bootc without changing the experience.

- **Check and download:** a system timer checks for a newer digest on every run, which costs a manifest, and then runs `bootc upgrade --download-only`, which stages a deployment locked against being applied. A shutdown the user did not confirm applies nothing. Only the download is skipped when NetworkManager reports the connection as metered (`Metered` 1 or 3); unknown and guessed-unmetered connections download, and so does a machine without NetworkManager.
- **A reboot discards a locked deployment;** the pulled image stays cached. The state therefore tells "available" from "downloaded", and "Restart to update" on a digest that is only available downloads it first.
- **Notice:** a user service sends **one** notification per staged digest and per user: an update is ready; actions "Restart to update" and "Later". "Later" leaves the deployment locked. No countdown, no repeat. The state is per machine; notifications are per user.
- **Confirmation is one step:** "Restart to update" asks the system side to unlock the deployment (`bootc upgrade --from-downloaded`) and reboot in the same request. The system side first asks logind whether a reboot is blocked by an inhibitor, and when it is, refuses without unlocking. An unlocked deployment therefore never waits for some later shutdown, the request grants a process nothing beyond the reboot logind already allows the active local user, and it takes the same polkit defaults.
- **After the first boot into a new deployment,** one notification says which version is now running, with the version and date from the image labels, and offers the way back. It claims no changelog: none exists yet.
- **Going back** is `bootc rollback` to the immediately previous deployment, behind administrator authentication, followed by a restart. A digest the user went back from is held: it is not downloaded again, and only a newer digest is offered. Without this rule the timer would undo the rollback. Stage 1 has no request that releases a held digest.

**SH12. The shield reports only what a verifier backs.**

- **The contract with the system side.** The shell never runs privileged code. It reads one root-owned, world-readable state file, written atomically by the system side, and sends two requests, apply and go back. Because every local process can read that file, the system side writes an enumerated error code and at most a host name into it, never a raw error string or a URL. It treats the file as untrusted input: every string is set as plain text, never as markup, truncated, and stripped of control and bidirectional characters. The words "verified" and "blocked" come only from the shell's own translations, never from the file.
- **Verified** means: the digest of the booted deployment carries a signature that the system side checked against a key still present in the policy in force. The result is recorded per digest and per key under `/var/lib`, so it survives the reboot. A deployment installed from the ISO reads "not verified: installed from media" until its first boot with a network, when the migration of `doc_update_trust.md`, UT4, moves it onto the signed digest without an update: the installer converts the image, the manifest digest changes, and no signature covers the new one (`doc_update_trust.md`, UT5). A digest fetched under a permissive policy, or whose key left the policy, is not verified. Nothing is inferred from the policy being strict today. The signature material is kept beside the record, so the result can be derived again and is not a bare boolean on a writable partition.
- **Three badges,** each different in shape and in colour so they read without colour vision:
  - a check: the booted image is verified, it is the newest the machine has booted, and the last successful check for updates is at most 14 days old;
  - an exclamation mark: not verified yet, the policy is not in force, the machine runs an older version than one it has booted, as after going back, or no check for updates has succeeded for 14 days, with that date. A machine whose updates stopped silently must not stay green: that defect has already shipped once (section 1), and an attacker who can only drop traffic causes it at will;
  - a cross: the last download was refused by the policy. It clears when a later download passes the policy.
- **Rows that do not move the badge.** Secure Boot off is the declared degraded mode of `doc_kernel_profile.md` D3: its row says so, plainly. The header of the sheet claims only what the rows back: "System image verified", never "this computer is safe". That sentence is about the image the deployment refers to. It is not a measurement of the running `/usr`: nothing ties the booted files to the digest until the dm-verity `/usr` of `doc_kernel_profile.md`, and no wording in the interface suggests otherwise.
- **The session shield is informative, not a trusted path.** A process running as the user can still draw over our bar, or replace the bar's user unit with a file or a drop-in of its own. The greeter's seal is the stronger one, because no user code runs there. A spoof-resistant shield needs the compositor work of SH2.
- **The shield is the Athanor mark with the badge.** There is no permanent text in the panel: the words are in the sheet and in the accessible name. Its place is the right end of the panel in every layout, and the top right corner of the greeter. Whether that place mirrors under RTL is decided in `doc_bar.md`; cosmic-panel does not mirror.
- **The session shield is a module of our bar** (package 2b). Its sheet carries the rows, "Restart to update" and "Go back to the previous version". It is a GTK popover of the bar if P4 shows that one opens from a top-level layer surface, and otherwise a layer-shell surface of its own, started by the bar. Spike P1 found the second shape forced inside cosmic-panel, which does not display a GTK popover (`surface missing from known popups`) and whose applets die of a second Wayland connection; the applet of package 1b-shield is therefore not built. Until 2b there is no session shield, and until the switch it exists only in a bar enabled by hand; meanwhile the trust state is reachable from the greeter's seal (package 1d) and from the notifier (`doc_update_trust.md`, UT11).

**The system side has its own spec, `doc_update_trust.md`,** written and passed through the `auditor` before package 1b-system is planned; the maintainer consents to it because it changes the signing pipeline. This document binds it to the following:

1. Release images gain a key-based cosign signature beside the keyless one, with the key in the `signing` environment.
2. The policy is `sigstoreSigned` with a `keyPaths` list, scoped to the project's registry only; `default` is untouched, so other registries keep working. The tools read only `/etc/containers/`, so the policy and the `registries.d` entry with `use-sigstore-attachments` live under `/usr` and the image build makes the `/etc` paths symbolic links to them. A local file that replaces a link shadows the policy; the state file names the policy actually in force, and the badge follows it.
3. A key rotation ships the new public key in an image still signed with the old one.
4. Existing installs move from `ostree-unverified-registry:` to a signed reference by an explicit, tested step.
5. The helper is a root D-Bus service with two methods and no arguments, apply and go back, one polkit action each in the `os.athanor.*` namespace, the subject taken from the bus sender through `athanor_bus_api::polkit`. Apply takes logind's defaults for a reboot (SH11); going back is `auth_admin` for every subject. Every new unit is hardened, and the user-side processes restrict themselves with Landlock at start, as the greeter does.
6. The state directory is declared in `tmpfiles.d` with owner and mode, apart from the directory that holds the released disk key.
7. "Secure Boot on" means `SecureBoot=1`, `SetupMode=0`, shim validation not disabled and kernel lockdown active, each published separately. The shipped `athanor-secure-boot` daemon cannot own its bus name today; `doc_update_trust.md`, D2, retires it and keeps the TPM files its package also ships.
8. Registry retention never removes an image, or the signature of an image, that a supported machine may still boot, download or go back to. `forge/scripts/clean_ghcr.sh` keeps two tagged versions per package today and counts signatures as versions.
9. Every published image carries a version label of its own. Today two builds a day apart are both `43.20260916.0`, and SH11 names the running version to the user.

**SH13. Tests.**

- **Layouts** are captured left-to-right in English with a fixed clock and an empty tray. The three presets at their factory knobs run outputs {1, 2} × scale {1.0, 1.5}: 12 cases. The other 11 layouts of SH7 run once at one output and scale 1.0. On one output taller than wide, at scale 1.0, the three presets at their factory knobs run once more, and so does `float` with the panel at the bottom, which stacks the dock above the panel: 27 layout cases. Until the switch of stage 2 they capture cosmic-panel; after it, our bar and dock, with the same 14 layouts and the same `layout.toml`.
- **Our own surfaces** run scale {1.0, 1.5} × theme {light, dark} × text {English, German for length, a right-to-left pseudo-locale}: 12 cases each. In stage 1 they are the greeter, with its seal from package 1d, and the chooser: 24 surface cases. The shield's 12 cases move to the bar; the bar and the dock add theirs in `doc_bar.md`. Italian and English are the shipped locales.
- **Where they run** (spike P3): in a rootless `fedora:43` container on the hosted `ubuntu-24.04` runner, with no GPU: a headless sway on pixman, cosmic-comp with its winit backend on llvmpipe, `cosmic-randr` for size and scale (an output taller than wide is set as a portrait size, not as a transform: only the shape matters, SH7), cosmic-panel, and `grim` over `ext-image-copy-capture-v1`. A scene costs 8 seconds and 1 GiB, and two independent runs are byte-identical.
- **Two outputs are not reachable there:** cosmic-comp has no headless backend and Smithay's winit backend has one output. The 6 two-output cases run as a scheduled job on the KVM runner. They do not gate a push, and they are not dropped: they are the ones that catch a per-output regression in the panel. A `vkms` device on the hosted runner may replace the KVM job; nobody has tried it.
- **What makes a golden reproducible:** isolated `XDG_*` directories per case, a frozen wall clock with a live monotonic clock, `TZ=UTC`, `LC_ALL` per case, a fixed set of running clients, the runner label `ubuntu-24.04` and the container pinned by digest. A case passes when at most a stated number of pixels differ from its golden image; the plan states the number.
- **There is no input in that environment,** so a surface is captured by starting it in the state under test. A surface with a sheet or a popover, such as the bar's shield, starts with it open.
- **What a golden cannot show:** the nested route has no dmabuf, no pointer, no hotplug, and reports a physical size of 0 × 0 mm. It proves layout and drawing, not behaviour on a real screen.
- Each surface has an automated accessibility check: every interactive widget exposes a role and a name in the AT-SPI tree.
- All strings go through gettext from the first commit.

## 3. Stages

### Stage 1

Four work packages, each with its own implementation plan. The first fixed two defects that shipped, updates that never ran and an image nobody verified, and depended on nothing in the shell.

| Package | Delivers | Delivered in |
|---|---|---|
| **1b-system. Updates and trust state** | `doc_update_trust.md`; key-based signature; policy; download timer in place of the broken preset line and override; state file; helper with its `.policy` and `system.d` files; the notifier, which needs neither GTK nor the tokens, so the timer never ships without a way to confirm | PR #64 |
| **1a. Design system** | tokens, generator, CI parse and contrast gates, default `CosmicTheme`, font, hearth wallpaper, seal icons, the greeter re-skinned on the tokens with roles and gettext | PR #55 |
| **1c. Layout** | schema, layered loader with migration and degradation, translator to cosmic-panel, first-session default, three presets with two knobs, a small chooser window | PR #63; its translator is a bridge to the switch of stage 2 (SH7) |
| **1d. The seal in the greeter** | the seal of SH12 in the greeter's top right corner, with the state file bound read-only into the greeter's sandbox; the greeter's 12 surface cases captured again | next; needs 1b-system and 1a, not stage 2 |

Package 1b-shield, a shield applet inside cosmic-panel, is not built: the session shield is born in our bar (SH12), and its seal in the greeter is package 1d.

### Stage 2

Three packages in this order, then one switch. Each package has its own implementation plan.

| Package | Delivers | Gated by |
|---|---|---|
| **2a. Compositor client** | `athanor-compositor-client` (SH2): windows, workspaces, outputs with their shape, actions and events in our own types; the reading of `CosmicTheme` moved into it from `athanor-style`; the boundary check in `scripts/verify.py`. It reads through GTK's `wl_display` (P4). `cosmic-client-toolkit` and `cosmic-protocols` are GPL-3.0-only, and every surface links this crate: 2a starts with the maintainer's decision on that licence. `cosmic-protocols` 0.2.0 has no keyboard-layout protocol, so the input-source module needs a newer release or another source | P4 |
| **2b. Bar** | our bar, a program in its own crate, drawing the three presets of `athanor-layout`, with the modules the switch requires: tray (StatusNotifier watcher and host), network, Bluetooth, audio, battery, power (shut down, restart, log out, lock), keyboard input source, clock, notifications (our server, its popups and a list in the bar), the shield (SH12), a launcher button that opens cosmic-launcher until stage 3, and the running applications in `bar` | 2a, `doc_bar.md` |
| **2c. Dock** | our dock, a program in its own crate (SH4) | 2b |

- **`doc_bar.md`** is written after P4 and passes a review before 2b is planned. It designs the modules, the shield's sheet, the tray, window titles in `bar`, the place of the shield under RTL, how applications are launched, and the surface cases of the bar and the dock.
- **Enabled by hand until the switch.** The bar runs as `athanor-bar.service`, a user unit, and the dock as a unit of its own. While the bar runs, cosmic-panel keeps drawing the dock (SH7), and our server alone owns `org.freedesktop.Notifications`: `athanor-cosmic-panel`, which today always starts cosmic-notifications and restarts the panel whenever the daemon exits, starts cosmic-panel without it. The translator writes only the dock entry, so the panel's notifications applet, the daemon's only peer, never starts; P4 (4) checks that cosmic-panel draws the dock without the socket pair. The translator and the wrapper read one signal, whether `athanor-bar.service` is enabled for the user, and act again when it changes. The layout document gains no key: enabling the bar is a development choice, not product surface.
- **One switch, at the end of 2c,** when the bar and the dock pass SH1's rule on the dev VM and on the maintainer's desktop. cosmic-panel, cosmic-applets and cosmic-notifications then leave the image, together with the unit and the wrapper `athanor-system-services` ships for them and its dependencies on them, the translator and the cosmic-panel module of `athanor-layout`. cosmic-launcher and cosmic-app-library stay until stage 3 if P4 shows that they work without the panel; otherwise the launcher of stage 3 comes before the switch.

Out of stage 2: our launcher, lock, polkit agent, on-screen display, Settings and overview; the headless daemons; the `split` preset; named outputs; releasing a held digest; a screen reader at the greeter; any compositor patch.

### Later stages

Stages 3 to 8 follow SH1 and SH3, each with its own spec. Stage 4 first checks whether cosmic-osd can run without its polkit agent, because a session holds one agent: if it cannot, the on-screen display comes forward from stage 5 into stage 4.

### Spikes

Each runs before the plan it gates and produces an answer, not code we keep.

| # | Spike | Settles |
|---|---|---|
| P1 | A GTK4 applet inside cosmic-panel: sizing, popover, scale, theme, memory; desktop-id shadowing from `~/.local/share`; overflow priority; behaviour when the panel restarts; the three badges at scale 1.0 on a low-density screen | **Yes, as two processes** (SH12). Not covered: fractional scale, a low-density physical screen, the overflow popup |
| P2 | Bump gtk4 0.7→0.11 and gtk4-layer-shell on the greeter, with and without relm4; count errors; list what the other crates need | **Upgrade relm4**; crates move together or leave the workspace (SH4) |
| P3 | cosmic-comp under llvmpipe in a container, two headless outputs, cosmic-panel running, one captured frame per output | **Hosted runner for one output, KVM for two** (SH13) |
| P4 | cosmic-comp 1.8 as the image ships it. (1) Which globals a client sees on the main socket and behind a security context, per engine id: today's exposure, since every application started outside a sandbox holds the main socket. (2) Inside a GTK4 process with the shim, whether the compositor client reaches windows, workspaces and outputs through GTK's `wl_display` or a second connection, which one survives the shim, and which `cosmic-client-toolkit` release matches the protocol versions of cosmic-comp 1.8. (3) Whether applications the bar and the dock start on a security-context socket (a GTK application, Firefox, a GL client) still work, and which globals they lose. (4) Whether cosmic-launcher and cosmic-app-library work, and open from their shortcut, with cosmic-panel stopped, and whether cosmic-panel draws the dock alone without the socket pair of cosmic-notifications. (5) Whether a GTK popover opens from a top-level layer surface, and the memory of a GTK4 bar on the Cairo renderer | **Run 2026-09-25; nothing in revision 5 changes.** (1) The main socket offers all 53 globals, the privileged ones included (toplevel manager, workspaces, output management, data control, virtual keyboard, input method, session lock, layer shell). Behind a security context a client sees 31, whatever the engine id, except `com.system76.CosmicPanel`, which cosmic-comp matches exactly and grants 51. Creating a context already needs the main socket, and a context offers no security-context manager, so neither path escalates. (2) Both paths work with the shim loaded and report the same windows, workspaces and outputs; the compositor client uses GTK's `wl_display`: one connection, no thread. `cosmic-client-toolkit` 0.2.0 resolves beside `gdk4-wayland` 0.11 on one `wayland-client` (0.31), and its toplevel-info, toplevel-manager, workspace and output versions equal cosmic-comp 1.8's; it lacks `zcosmic_keyboard_layout_manager_v1`, and it is GPL-3.0-only (2a). (3) A GTK application, an EGL client and Firefox map and draw on an `athanor.bar` context; they lose the 22 privileged globals, layer shell included. (4) With cosmic-panel stopped, Super opens cosmic-launcher and Super+A cosmic-app-library, as with the panel running; on a cold start the first press only starts the process, with or without the panel. cosmic-panel with the dock entry alone, no cosmic-notifications and no socket pair, stays up, draws the dock and logs one failed connection to the daemon. The launcher and the app library therefore need not leave with the panel. (5) A popover opens from a top-level layer surface. A bar of one button and one popover on the Cairo renderer holds 38 MB RSS and 16 MB PSS, 43 and 18 MB with the popover open; `doc_bar.md` measures the real modules |

P4 replaces the two spikes revision 4 reserved for stage 2: the security-context privilege model and a side-connection client for toplevels, workspaces and capture.

## 4. Risks

- **The shim.** GTK 4.16 broke `gtk4-layer-shell` once. SH4 bounds the damage to one call site per surface. The greeter is exposed in stage 1, and the bar and the dock join it in stage 2.
- **A wrong signature policy blocks updates,** not boot. It is proven on the dev VM against a signed image, the same image without its signature and an image signed with another key, before it reaches the image. Going back stays available.
- **Two editors of the panel configuration.** COSMIC Settings keeps its Panel and Dock pages. An edit there lasts until the next layout change, when the translator rewrites the configuration. Accepted until the switch of stage 2, when it ends.
- **cosmic-panel moves fast.** The translator targets a configuration format we do not own. The 27 layout cases run against every COSMIC bump.
- **The translator is throwaway.** It dies with cosmic-panel, at the switch of stage 2. The schema, the loader and the presets are the permanent part, and they are what the tests of 1c protect first.
- **Scope.** Stage 2 does again what COSMIC's applets do today, each a client of NetworkManager, BlueZ, PipeWire, UPower or another service, and the stages after it re-solve the rest of a desktop, for one maintainer. The brake is SH1: a surface is enabled by hand until it passes the replacement rule, so an average user keeps COSMIC's until then, and each package ships alone and is useful alone.
- **The compositor client is the one place that knows COSMIC.** A surface that reaches past it makes the exit of SH2 costlier without anyone noticing; the check of `scripts/verify.py` turns that into a failure.

## 5. Open doubts

1. **A shield applet in cosmic-panel could be shadowed or removed** (section 1): P1 confirmed that the panel runs an applet from the user's data directory. The doubt is closed for the shield, which is not built as an applet (SH12). At the switch desktop-id shadowing no longer applies at all; a user unit or drop-in can still replace the bar, so the session shield stays informative (SH12).
2. **Where the verification runs.** `skopeo standalone-verify`, `cosign verify` with the key, or the pull itself under the policy: `doc_update_trust.md` picks one and says what it needs offline.
3. **The metered state** is "unknown" on many networks, so SH11 downloads there. A user on an unmarked tethered phone pays for it; the rule errs towards being up to date.
4. **The sheet's behaviour** is designed in `doc_bar.md`: dismissal on an outside click or on focus loss, its stacking against the panel's own popups, which it drew over in the spike, and its place when the panel is at the bottom.
5. **Hiding COSMIC Settings pages** that configure a panel we later remove may need a patch. It is a problem of stage 6, when Settings becomes ours.
6. **The greeter holds the real Wayland socket,** with capture and clipboard privilege. Per-surface confinement needs the compositor work of SH2. P4 measures what every client on that socket can do.
7. **A screen reader at the greeter** needs an accessibility bus, Orca and audio for the `greetd` user. Stage 1 delivers the roles and names; the plumbing is a later requirement of "for everyone", not a wish.
8. **Inter** is a proposal from the mockups, not yet seen on real hardware at fractional scale.
9. **Multi-user and kiosk machines** are covered by wording (per-user notices and picks, SH9.1 without a panel), not yet by a test.
10. **How often a user is asked to restart: settled on 2026-09-19.** `doc_update_trust.md`, D1: users follow a `stable` tag that the maintainer promotes by hand, and `latest` stays for testing.
11. **Outputs taller than wide: settled on 2026-09-24.** The fourteen layouts in both shapes, and two outputs of different shape side by side, were drawn and approved by the maintainer (`.superpowers/brainstorm/89841-1789751828/content/layout-orientations.html`). cosmic-panel 1.8.0 on cosmic-comp 1.8.0 stacks the dock above the panel when both are anchored to the bottom edge with an exclusive zone; this was checked in a nested cosmic-comp (tier A of `scripts/devvm`) with the packages the image ships.

## 6. Changes to other documents

- `doc_shell_ui.md` describes niri, relm4 and `athanor-settings-rs`. This document replaces it: the file is deleted, and the links to it in `README.md` and `forge/README.md` point here.
- `doc_update_trust.md` is a new document (SH12).
- `doc_platform_experience.md`, section 3, names "the native GTK4/Relm4 panel and the horizontal strip of `Niri`". It takes a pointer to this document.
- `doc_kernel_profile.md`, the note on the existing override that "stages updates automatically": it takes a pointer to SH11, which removes that override.
- `NEXT.md` takes stage 1 as a block with the gate of section 7, and stage 2 as a second block with the gate of section 8.
- `doc_bar.md` is a new document, written after spike P4 (section 3).
- `CLAUDE.md`, "Desktop GTK4/Wayland": unchanged.

## 7. Acceptance of stage 1

On a fresh install in the dev VM, and on the maintainer's desktop upgraded in place:

1. The generated CSS loads with zero GTK parse warnings, and the contrast gate passes in the four variants.
2. On the fresh install the greeter, COSMIC's surfaces and COSMIC applications show one identity in light and in dark. On the upgraded desktop the existing theme is untouched. Changing the accent in COSMIC Settings changes our surfaces, and the trust colours do not move.
3. The AT-SPI tree of the greeter, with its seal, and of the chooser exposes a role and a name for every control, checked in the test rig, where an accessibility bus exists; both run in Italian and in English. The session shield is acceptance of stage 2 (section 8).
4. With a newer image published, the download happens with no user action and one notification appears. A shutdown after "Later" boots the same version, and "Later" is not asked again in that session; while no shield exists, the notifier offers a pending digest once per session start (`doc_update_trust.md`, UT11). On a connection marked metered the check runs and the download does not.
5. "Restart to update" boots the new deployment, and one notification names the running version.
6. "Go back to the previous version" asks for administrator authentication and, after the restart, the old digest is booted, the badge is the exclamation mark, and the timer does not download the held digest again.
7. The same image without its signature, and an image signed with another key, are refused; the badge is the cross, never the check.
8. A signed image reads "verified" after the restart that boots it. The fresh install reads "not verified: installed from media" until the migration has run and "verified" after it, never the check by default. With the clock moved 15 days past the last successful check and the network down, the badge is the exclamation mark.
9. The upgraded desktop moves from `ostree-unverified-registry:` to the signed reference by the documented step and keeps updating. On the dev VM, an image that carries a second public key and is signed with the first is accepted, and so is the next one, signed with the second.
10. The three presets and the two knobs apply from the chooser without a restart of the session. A user document with an unknown key degrades to a preset and the file is byte-identical afterwards. A key marked mandatory in `/etc` is greyed in the chooser. A new user on a screen under 800 logical pixels gets `bar`, and one with an output taller than wide gets `float`. Turning an output a quarter moves a side dock to the bottom edge without a restart of the session. A translator made to fail repeatedly leaves the vendor layout on screen, not an empty one.
11. The 21 single-output layout cases and the 24 surface cases pass in CI, and the 6 two-output layout cases pass on the scheduled KVM job.

## 8. Acceptance of stage 2

After the switch, on a fresh install in the dev VM and on the maintainer's desktop upgraded in place:

1. The boundary check of `scripts/verify.py` passes with only `forge/tools/calmo-cosmic-theme` excepted; the translator and the cosmic-panel files of `athanor-layout` are gone.
2. cosmic-panel, cosmic-applets and cosmic-notifications are not in the image; `org.kde.StatusNotifierWatcher` and `org.freedesktop.Notifications` are owned by our processes.
3. Every module of package 2b works against the real service: a StatusNotifier client shows its icon and its menu in the tray, `notify-send` shows a notification, the network, Bluetooth, audio, power and input-source controls change the system state they show, on hardware that has it, and the battery module shows what UPower reports. No module is a facade (SH1).
4. The three presets and the two knobs apply from the chooser without a restart of the session, drawn by our bar and dock, with the `layout.toml` of stage 1 unchanged.
5. No layout document, user or policy, removes the shield from the bar (SH9.1). The shield shows the badge the state file backs, and its sheet offers "Restart to update" and "Go back to the previous version" as SH11 and SH12 define them.
6. The AT-SPI tree of the bar and of the dock exposes a role and a name for every control; Orca reads the shield; both run in Italian and in English.
7. Against our surfaces, the 21 single-output layout cases pass in CI and the 6 two-output cases on the KVM job, and the surface cases of the bar and the dock in `doc_bar.md` pass in CI.

`doc_bar.md` refines these criteria and may add to them; it removes none.
