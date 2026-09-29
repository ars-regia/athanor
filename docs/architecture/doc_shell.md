# Athanor shell: direction, layout model, stage 1

Status: **approved on 2026-09-18, revision 3; revision 4, approved by the maintainer on 2026-09-24, adds outputs taller than wide to SH7, SH10, SH13 and sections 4, 5 and 7.** The maintainer took the decisions below and delegated the validation. Revision 1 failed two independent reviews, an adversarial one and a security one: the direction held, the update and trust part did not. Revision 2 took their findings, both reviewers then found it approvable with changes, and revision 3 takes those changes. The four reports are `.superpowers/shell-spec-review.md`, `shell-spec-security-review.md` and their `-r2` successors.

The approval covers the direction and packages 1a, 1b-shield and 1c. Package 1b-system starts only when `doc_update_trust.md` exists, has passed the `auditor` and has the maintainer's consent, because it changes the signing pipeline (SH12). Section 5 lists what is still unverified; doubt 10 waits for a maintainer decision.

The three spikes of section 3 ran on 2026-09-18 (`.superpowers/spike-p1-applet.md`, `spike-p2-gtk-bump.md`, `spike-p3-headless.md`). Their results are written into SH4, SH5, SH12 and SH13 below; none overturned a decision.

The document replaces `doc_shell_ui.md` and amends `doc_platform_experience.md`, section 3. Section 6 lists the changes other documents take.

## 1. Context

Since 2026-09-11 the desktop is Fedora's COSMIC 1.8 on cosmic-comp: panel, dock, launcher, settings, notifications. `athanor-shell-rs` survives only as the greeter. The maintainer wants a proprietary shell back, evolved rather than restored, because the graphical environment is what an average user judges the system by.

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

## 2. Decisions

**SH1. Staged path.** COSMIC stays underneath and its surfaces are replaced one at a time.

- **Stage 1:** a real design system, the update experience and the trust shield, on top of COSMIC. Section 3 scopes it.
- **Later stages:** one surface per stage, in this order unless a stage's own spec argues otherwise: session lock, notifications, panel with tray, launcher, overview.
- **The rule for replacing a surface:** ours is usable by an average user and better than COSMIC's at the moment of the switch. Tray, fractional scaling, screen-reader roles and i18n are requirements of every surface, not extras.
- **No facades.** A control that does nothing, or a reading with no source, is a defect.

**SH2. cosmic-comp stays; the dependency has an exit.** We do not write a compositor.

- A Rust compositor is 2.6–3.3 MB of Rust on top of Smithay's 4.4 MB, and its cost is the hardware matrix, not the first version. A compositor of our own on Smithay would still depend on the maintainer of cosmic-comp, through the library.
- cosmic-comp is GPL-3 and Smithay is MIT. A fork is always possible, and we only run cosmic-comp as a separate process.
- **Containment:** the `cosmic-*` protocols are used from one module of the shell and nowhere else. Where a standard protocol exists (`ext-workspace-v1`, `ext-foreign-toplevel-list-v1`, `ext-image-copy-capture-v1`, `ext-session-lock-v1`), the shell uses the standard one.
- **Security features only a compositor can give** (authenticated privileged clients, a trusted path for credential prompts, compositor-drawn trust decorations) are proposed upstream first. If upstream declines, they become a small isolated patch set built in the forge and rebased per release.
- **A fork is reconsidered only when** System76 abandons or relicenses cosmic-comp, or a security requirement we cannot drop is declined and the patch set stops being maintainable, or the project has more maintainers. A fork starts from cosmic-comp, never from zero.

**SH3. COSMIC's headless daemons stay by choice.** The shell owns pixels, not plumbing.

| Component | Role | Fate |
|---|---|---|
| cosmic-settings-daemon | configuration bus, media keys | stays |
| cosmic-settings, cosmic-randr | Settings application | stays; `athanor-settings-rs` is not revived |
| cosmic-osd | OSD and the session's only polkit agent | stays; replacing it means writing a polkit agent |
| cosmic-idle | `org.freedesktop.ScreenSaver`, idle policy | stays |
| cosmic-greeter as locker | session lock | first surface to be replaced |
| cosmic-notifications | notification daemon, coupled to the panel by an inherited socket pair | leaves with the panel; our notification server returns |
| cosmic-panel, cosmic-applets, cosmic-launcher, cosmic-app-library | panel, dock, launcher | leave when our panel provides the StatusNotifier watcher and host, and the network, Bluetooth, audio, battery, power and input-source modules |
| cosmic-workspaces, cosmic-bg | overview, wallpaper | stay until replaced |

COSMIC applications (`cosmic-files`, `cosmic-term`, `cosmic-edit`, `cosmic-store`) are content, not dependencies. They read `CosmicTheme`, so the design system writes it (SH5).

**SH4. GTK4, with the shim kept replaceable.**

- Our surfaces use GTK4 through plain `gtk4-rs`. AT-SPI, IME and RTL work today.
- **One crate per program** (maintainer's decision, 2026-09-19). Every surface is its own binary in its own crate, with only the dependencies, the Landlock rules and the review surface it needs: the greeter first, because it handles the password. Shared code lives in a library crate only when two programs really use it: `athanor-style` for tokens and CSS, `athanor-trust-state` for the state file and the badge, later one crate for the `cosmic-*` protocols. `athanor-shell-rs`, the single binary that holds the greeter beside 13,000 lines of surfaces this document calls dead, is not bumped: it leaves the workspace frozen at GTK 0.7 and is mined for the code worth keeping until it is empty, then deleted.
- **relm4 is upgraded, not dropped** (spike P2, on `iso-v0` at cfe61ecf). With gtk4 0.11.4, gtk4-layer-shell 0.8.1 and relm4 0.11.0 the shell crate has 54 errors, none caused by relm4: 25 old-syntax `glib::clone!`, 20 `Option` wraps, 4 removed channel APIs that need `async-channel`, 5 others. That is a day to a day and a half, and it is not spent: the greeter uses no relm4, needs one line, and moves to its own crate.
- **The GTK crates move together or leave the workspace.** `gtk4-sys` declares `links = "gtk-4"`, so no crate can be pinned apart. Nine crates touch gtk4, two of them only through `athanor-style`; two ship today, `athanor-shell-rs` and `athanor-recovery`, and `athanor-dock` is a path dependency of the first. The plan of 1a proposes which unshipped crates leave `[workspace.members]` before the bump; the maintainer decides, because it retires code.
- `gtk4-layer-shell` is a one-maintainer symbol-interposition shim that GTK does not support. Therefore: one process per surface, all logic in toolkit-agnostic crates, and the shim called from one place per surface. If the shim fails, only the panel moves to a direct `wayland-rs` client. Qt is not a fallback.
- **The shim degrades silently,** to a normal window with a title bar, when it is not loaded before `libwayland-client` (spike P3). Our Rust binary links it first today, by the accident of link-flag order and not by contract (spike P2). Therefore the package's `%check` asserts the `DT_NEEDED` order, and every layer-shell surface asserts at start that it is a layer surface and exits with an error when it is not.
- **Surfaces that live in the panel use GTK's Cairo renderer.** With the default renderer an icon-sized applet maps 216–320 MB; with Cairo it is 41 MB PSS, beside 33 MB for a stock applet (spike P1).
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
- **The policy layer is not a security boundary,** and no wording in the interface says it is. It sets defaults, and a key it marks mandatory is honoured by our loader and greyed in our chooser. While cosmic-panel reads a user-writable configuration, a user can still move the panel behind our back; enforcement arrives with our own panel.
- The document names a preset and the knobs of SH7, nothing else. It is the serialisation of what the interface offers, never a superset. The accent is not in it: in stage 1 the accent belongs to the theme (SH5).

**SH7. Presets are data.** A preset is a style plus factory values for the knobs. Version 1 ships three presets and two knobs.

| Id | Display name | Style | Factory panel | Factory dock |
|---|---|---|---|---|
| `float` | Isola | floating rounded panel (COSMIC 1.8.0 ships an edge-to-edge one), floating centred dock as COSMIC ships it | top | visible |
| `bar` | Barra | one edge-to-edge bar holding launcher, running applications, tray and clock | bottom | no knob |
| `minimal` | Essenziale | thin edge-to-edge bar | top | none |

- **Identifiers are permanent and English; display names are translated.** A fourth preset, three separate islands with a vertical dock (`split`), needs our own panel and is not in version 1.
- **Knobs:** panel position (top, bottom); dock (visible, auto-hide, none).
- **A knob exists only where it means something.** `bar` holds the applications in the bar, so it has no dock knob and the schema rejects one. That leaves 2 × 3 layouts for `float`, 2 for `bar` and 2 × 3 for `minimal`: 14 in version 1.
- **Dock placement is derived, not chosen,** from the panel position and from the shape of each output. On an output wider than tall the dock sits on the bottom edge, and on the left edge when the panel is at the bottom. On an output taller than wide it never takes a side edge, which would spend the scarce dimension: it sits on the bottom edge, stacked above the panel when the panel is at the bottom. A vertical dock carries icons only (SH9.3).
- **Rotation matters only through the shape.** A flipped or mirrored output keeps its logical geometry and takes the layout of the unrotated one; a quarter turn (90° or 270°) makes it taller than wide. The document stays keyed by `[output."*"]`: the shape is read from each output, never stored.
- **`bar` in stage 1 is an icon taskbar:** no COSMIC applet renders window titles. Titles wait for our panel and for a decision on who may read them. `minimal` is subject to cosmic-panel#560 being absent on 1.8.
- In stage 1 a translator turns the effective document into cosmic-panel configuration. It writes `entries` last, because that configuration is live and not atomic. It runs when the effective document changes and when an output is added, removed or rotated, because the dock's edge depends on each output's shape; it therefore stays resident in the session and watches the outputs, but it rewrites nothing at login unless the result differs, and it is idempotent. It writes one dock for every output while all of them share a shape, and one dock per output otherwise. cosmic-panel's configuration is not the user's layout document: SH8's promise covers `layout.toml`, not the files under `~/.config/cosmic`.

**SH8. The schema is the ratchet.** Everything the schema can express is supported and tested; nothing else is expressible.

- **Reading:** the shell reads every schema version it ever shipped and migrates in memory. A preset or key removed in a later version maps to a named successor in a migration table shipped with the schema.
- **Rejecting:** an unknown key, a newer `schema`, or a malformed file rejects the whole user document. The shell then applies the nearest preset it knows and logs at error priority. A rollback to an older `/usr` therefore degrades, and a later upgrade restores.
- **Writing:** the shell writes the user document only when the user changes the layout, at the current schema. It never rewrites it on its own. When the rejected document has a newer schema, the chooser asks before saving and keeps the old file as `layout.toml.<schema>`.
- **Crash-loop protection** follows the policy of `/usr/bin/athanor-cosmic-panel`: bounded failures in a `CLOCK_BOOTTIME` window, then the vendor layout. The shell is never lost.

**SH9. Invariants, never configurable.**

1. The trust shield is present whenever a panel is present, at the same place within a preset. Without a panel the trust state stays reachable from the greeter and from the update notifications. Stage 1 cannot enforce this against the user's own session (SH12).
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
- **Rows that do not move the badge.** Secure Boot off is the declared degraded mode of `doc_kernel_profile.md` D3: its row says so, plainly. The header of the popover claims only what the rows back: "System image verified", never "this computer is safe". That sentence is about the image the deployment refers to. It is not a measurement of the running `/usr`: nothing ties the booted files to the digest until the dm-verity `/usr` of `doc_kernel_profile.md`, and no wording in the interface suggests otherwise.
- **In stage 1 the session shield is informative, not a trusted path.** A process running as the user can shadow, remove or overdraw it. The greeter's seal is the stronger one, because no user code runs there. A spoof-resistant shield needs the compositor work of SH2.
- **The shield is the Athanor mark with the badge.** There is no permanent text in the panel: the words are in the popover and in the accessible name. Its place is the right end of the panel in every layout, and the top right corner of the greeter. Whether that place mirrors under RTL is decided with our own panel; cosmic-panel does not mirror.
- **In stage 1 the shield is two processes** (spike P1). The seal is a GTK4 applet inside cosmic-panel, which starts it, sizes it, hands it clicks and restarts it when it dies. Its sheet is a separate layer-shell surface on the compositor, started by the seal, carrying the rows, "Restart to update" and "Go back to the previous version". A GTK popover cannot be used: cosmic-panel 1.8 does not display one (`surface missing from known popups`), and a second Wayland connection inside the applet kills it through the shim.

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

- **Layouts** are captured left-to-right in English with a fixed clock and an empty tray. The three presets at their factory knobs run outputs {1, 2} × scale {1.0, 1.5}: 12 cases. The other 11 layouts of SH7 run once at one output and scale 1.0. On one output taller than wide, at scale 1.0, the three presets at their factory knobs run once more, and so does `float` with the panel at the bottom, which stacks the dock above the panel: 27 layout cases.
- **Our own surfaces** (greeter, shield popover, chooser) run scale {1.0, 1.5} × theme {light, dark} × text {English, German for length, a right-to-left pseudo-locale}: 36 surface cases. Italian and English are the shipped locales.
- **Where they run** (spike P3): in a rootless `fedora:43` container on the hosted `ubuntu-24.04` runner, with no GPU: a headless sway on pixman, cosmic-comp with its winit backend on llvmpipe, `cosmic-randr` for size and scale (an output taller than wide is set as a portrait size, not as a transform: only the shape matters, SH7), cosmic-panel, and `grim` over `ext-image-copy-capture-v1`. A scene costs 8 seconds and 1 GiB, and two independent runs are byte-identical.
- **Two outputs are not reachable there:** cosmic-comp has no headless backend and Smithay's winit backend has one output. The 6 two-output cases run as a scheduled job on the KVM runner. They do not gate a push, and they are not dropped: they are the ones that catch a per-output regression in cosmic-panel. A `vkms` device on the hosted runner may replace the KVM job; nobody has tried it.
- **What makes a golden reproducible:** isolated `XDG_*` directories per case, a frozen wall clock with a live monotonic clock, `TZ=UTC`, `LC_ALL` per case, a fixed set of running clients, the runner label `ubuntu-24.04` and the container pinned by digest. A case passes when at most a stated number of pixels differ from its golden image; the plan states the number.
- **There is no input in that environment,** so a surface is captured by starting it in the state under test. The shield's sheet, being its own process, starts open.
- **What a golden cannot show:** the nested route has no dmabuf, no pointer, no hotplug, and reports a physical size of 0 × 0 mm. It proves layout and drawing, not behaviour on a real screen.
- Each surface has an automated accessibility check: every interactive widget exposes a role and a name in the AT-SPI tree.
- All strings go through gettext from the first commit.

## 3. Stage 1

Four work packages, each with its own implementation plan, in this order. The first fixes two defects that ship today, updates that never run and an image nobody verifies, and depends on nothing in the shell.

| Package | Delivers | Gated by |
|---|---|---|
| **1b-system. Updates and trust state** | `doc_update_trust.md`; key-based signature; policy; download timer in place of the broken preset line and override; state file; helper with its `.policy` and `system.d` files; the notifier, which needs neither GTK nor the tokens, so the timer never ships without a way to confirm | the maintainer's consent to that spec |
| **1a. Design system** | tokens, generator, CI parse and contrast gates, default `CosmicTheme`, font, hearth wallpaper, seal icons, the greeter re-skinned on the tokens with roles and gettext | P2, P3 |
| **1b-shield. Shield** | shield applet with its popover, the seal in the greeter, with the state file bound read-only into the greeter's sandbox | 1b-system, 1a, P1 |
| **1c. Layout** | schema, layered loader with migration and degradation, translator to cosmic-panel, first-session default, three presets with two knobs, a small chooser window | 1a, P3 |

Spikes, run before the plan they gate; each produced an answer, not code we keep:

| # | Spike | Settles |
|---|---|---|
| P1 | A GTK4 applet inside cosmic-panel: sizing, popover, scale, theme, memory; desktop-id shadowing from `~/.local/share`; overflow priority; behaviour when the panel restarts; the three badges at scale 1.0 on a low-density screen | **Yes, as two processes** (SH12). Not covered: fractional scale, a low-density physical screen, the overflow popup |
| P2 | Bump gtk4 0.7→0.11 and gtk4-layer-shell on the greeter, with and without relm4; count errors; list what the other crates need | **Upgrade relm4**; crates move together or leave the workspace (SH4) |
| P3 | cosmic-comp under llvmpipe in a container, two headless outputs, cosmic-panel running, one captured frame per output | **Hosted runner for one output, KVM for two** (SH13) |

Two more spikes gate stage 2, not stage 1: the security-context privilege model on 1.8, and a side-connection client for toplevels, workspaces and capture.

Out of stage 1: our own panel, dock, launcher, notifications, lock and Settings; the `split` preset; window titles in `bar`; a curated accent palette; named outputs; releasing a held digest; a screen reader at the greeter, which belongs to the stage that replaces the session lock; any compositor patch.

## 4. Risks

- **The shim.** GTK 4.16 broke `gtk4-layer-shell` once. SH4 bounds the damage to one call site per surface; the greeter is the exposed surface in stage 1.
- **A wrong signature policy blocks updates,** not boot. It is proven on the dev VM against a signed image, the same image without its signature and an image signed with another key, before it reaches the image. Going back stays available.
- **Two editors of the panel configuration.** COSMIC Settings keeps its Panel and Dock pages, where the user can also remove the shield. An edit there lasts until the next layout change, when the translator rewrites the configuration. Accepted for stage 1; it ends with our own panel.
- **cosmic-panel moves fast.** The translator targets a configuration format we do not own. The 27 layout cases run against every COSMIC bump.
- **The translator is throwaway.** It dies with cosmic-panel. The schema, the loader and the presets are the permanent part, and they are what the tests of 1c protect first.
- **Scope.** Stage 1 is four packages for one maintainer and an agent. They ship one at a time, and each is useful alone. Each later stage re-solves something COSMIC already solved; SH1's replacement rule is the brake.

## 5. Open doubts

1. **The session shield can be shadowed or removed** (section 1, SH12); P1 confirmed that the panel runs an applet from the user's data directory. Whether the panel unit can run with a restricted data path without breaking the application list is still untested.
2. **Where the verification runs.** `skopeo standalone-verify`, `cosign verify` with the key, or the pull itself under the policy: `doc_update_trust.md` picks one and says what it needs offline.
3. **The metered state** is "unknown" on many networks, so SH11 downloads there. A user on an unmarked tethered phone pays for it; the rule errs towards being up to date.
4. **The sheet's behaviour is not designed yet:** dismissal on an outside click or on focus loss, its stacking against the panel's own popups, which it drew over in the spike, and its place when the panel is at the bottom.
5. **Hiding COSMIC Settings pages** that configure a panel we later remove may need a patch. Not a stage 1 problem.
6. **The greeter holds the real Wayland socket,** with capture and clipboard privilege. Per-surface confinement needs the compositor work of SH2.
7. **A screen reader at the greeter** needs an accessibility bus, Orca and audio for the `greetd` user. Stage 1 delivers the roles and names; the plumbing is a later requirement of "for everyone", not a wish.
8. **Inter** is a proposal from the mockups, not yet seen on real hardware at fractional scale.
9. **Multi-user and kiosk machines** are covered by wording (per-user notices and picks, SH9.1 without a panel), not yet by a test.
10. **How often a user is asked to restart.** The Orchestrator builds every night, and "one notice per digest" then means one notice a day. A promoted `stable` tag that users follow, with `latest` kept for testing, would fix it; `doc_kernel_profile.md` leaves channels to release 1.1. The maintainer decides whether one hand-promoted channel comes forward into `doc_update_trust.md`.
11. **Outputs taller than wide: settled on 2026-09-24.** The fourteen layouts in both shapes, and two outputs of different shape side by side, were drawn and approved by the maintainer (`.superpowers/brainstorm/89841-1789751828/content/layout-orientations.html`). cosmic-panel 1.8.0 on cosmic-comp 1.8.0 stacks the dock above the panel when both are anchored to the bottom edge with an exclusive zone; this was checked in a nested cosmic-comp (tier A of `scripts/devvm`) with the packages the image ships.

## 6. Changes to other documents

- `doc_shell_ui.md` describes niri, relm4 and `athanor-settings-rs`. This document replaces it: the file is deleted, and the links to it in `README.md` and `forge/README.md` point here.
- `doc_update_trust.md` is a new document (SH12).
- `doc_platform_experience.md`, section 3, names "the native GTK4/Relm4 panel and the horizontal strip of `Niri`". It takes a pointer to this document.
- `doc_kernel_profile.md`, the note on the existing override that "stages updates automatically": it takes a pointer to SH11, which removes that override.
- `NEXT.md` takes stage 1 as a block with the gate of section 7.
- `CLAUDE.md`, "Desktop GTK4/Wayland": unchanged.

## 7. Acceptance of stage 1

On a fresh install in the dev VM, and on the maintainer's desktop upgraded in place:

1. The generated CSS loads with zero GTK parse warnings, and the contrast gate passes in the four variants.
2. On the fresh install the greeter, COSMIC's surfaces and COSMIC applications show one identity in light and in dark. On the upgraded desktop the existing theme is untouched. Changing the accent in COSMIC Settings changes our surfaces, and the trust colours do not move.
3. The AT-SPI tree of the greeter, of the shield and of the chooser exposes a role and a name for every control, checked in the test rig, where an accessibility bus exists; Orca reads the shield in the user session; all three run in Italian and in English.
4. With a newer image published, the download happens with no user action and one notification appears. A shutdown after "Later" boots the same version, and "Later" is not asked again in that session; while no shield exists, the notifier offers a pending digest once per session start (`doc_update_trust.md`, UT11). On a connection marked metered the check runs and the download does not.
5. "Restart to update" boots the new deployment, and one notification names the running version.
6. "Go back to the previous version" asks for administrator authentication and, after the restart, the old digest is booted, the badge is the exclamation mark, and the timer does not download the held digest again.
7. The same image without its signature, and an image signed with another key, are refused; the badge is the cross, never the check.
8. A signed image reads "verified" after the restart that boots it. The fresh install reads "not verified: installed from media" until the migration has run and "verified" after it, never the check by default. With the clock moved 15 days past the last successful check and the network down, the badge is the exclamation mark.
9. The upgraded desktop moves from `ostree-unverified-registry:` to the signed reference by the documented step and keeps updating. On the dev VM, an image that carries a second public key and is signed with the first is accepted, and so is the next one, signed with the second.
10. The three presets and the two knobs apply from the chooser without a restart of the session. A user document with an unknown key degrades to a preset and the file is byte-identical afterwards. A key marked mandatory in `/etc` is greyed in the chooser. A new user on a screen under 800 logical pixels gets `bar`, and one with an output taller than wide gets `float`. Turning an output a quarter moves a side dock to the bottom edge without a restart of the session. A translator made to fail repeatedly leaves the vendor layout on screen, not an empty one.
11. The 27 layout cases and the 36 surface cases pass in CI.
