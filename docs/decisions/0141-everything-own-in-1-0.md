---
id: ADR-0141
title: "Release 1.0 is entirely Athanor's own: what counts as own, and the scope it sets"
date: 2026-11-18
status: accepted
issues: []
areas: [shell, security, apps, process, docs]
---

# 0141. Release 1.0 is entirely Athanor's own: what counts as own, and the scope it sets

**Accepted** by the maintainer with the pull request that records it. On 2026-11-18 the maintainer
said (translated from Italian): "for me 1.0 must have everything of our own; can we define best what
this means?" On the same day the maintainer answered the three questions of section "Maintainer's
answers", and the decisions below follow those answers. Approving this record approves the
decisions below.

## Context

[ADR-0121](0121-own-compositor-and-toolkit.md) decision 5 ships 1.0 on Sulphur "with the GTK4
shell on top". [ADR-0135](0135-faster-road-to-1-0.md) decision 3 keeps cosmic-comp's session at
the greeter as the second choice. It also moves to 1.1:
- the overview;
- the retirement of cosmic-workspaces;
- the full acceptance of `doc_own_stack.md` OS19.

On 2026-11-06 the maintainer said "we must replace everything that is not our own", and
[ADR-0138](0138-own-components-in-1-0.md) placed in 1.0, written in Sal:
- the polkit agent;
- the lock;
- the window switcher;
- the session daemons;
- the output management.

`docs/operations/release-1.0.md` still says that the bar, the dock, the control center, the
greeter UI and the layout chooser "stay GTK4". That sentence is already stale for the bar, the
control center and the layout chooser, which build without GTK today. No record says what "own"
means, so no check can tell when 1.0 meets it.

### What is not own at `a43bf06d9`

**Upstream packages in the image** (`forge/config/packages.json`, `upstream_desktop`):
- cosmic-app-library;
- cosmic-greeter;
- cosmic-icon-theme;
- cosmic-launcher;
- cosmic-osd;
- cosmic-randr;
- cosmic-settings;
- cosmic-settings-daemon;
- cosmic-store;
- cosmic-workspaces;
- pop-launcher;
- gtk-layer-shell and gtk4-layer-shell;
- xdg-user-dirs-gtk.

**Forked package:** cosmic-comp, built in tier 2, ships as the greeter's second session.

**Crates that still depend on `gtk4`:**
- `athanor-greeter-ui`;
- `athanor-recovery`;
- `athanor-dock`, behind a feature;
- `athanor-apps`, `athanor-controls`, `athanor-style` and `athanor-sulphur-client`.

Of these, `rig.sh no-gtk` already builds `athanor-apps`, `athanor-style`, the compositor client,
`athanor-dock` and `athanor-osd` without GTK. `rig.sh build-bar` and `rig.sh build-control-center` check that
no GTK library is in the bar's or the control center's `DT_NEEDED`.

**Specifications** that still name libadwaita for Athanor's applications:
- `doc_files.md` FM1 and FM2;
- `doc_settings.md`;
- `doc_accessibility.md`, `doc_disks.md`, `doc_first_run.md`, `doc_languages.md`, `doc_portal.md`,
  `doc_session_daemons.md`, `doc_shell.md`, `doc_software.md`, `doc_virtualization.md` and
  `doc_visual_language.md`.

## Decision

1. **A component is Athanor's own when all three of these hold:**
   - its source is in this repository and Athanor maintains it;
   - it draws with Sal and runs on Sulphur, with no GTK, libadwaita, libcosmic or iced in its
     dependency tree;
   - it carries no code copied from COSMIC, so its licence stays Athanor's (ADR-0121 decision 2).

   A component is a program, a service, a session surface or a first-party application.
   Libraries are not components: Smithay, wayland-rs, zbus, Parley, AccessKit, fontconfig, the
   Rust crates of `deny.toml`. A library is taken under `deny.toml` and its licence rules. In
   English the word is "own", never "proprietary": Athanor's code is open.

2. **What must be own in 1.0, layer by layer.**

   | Layer | 1.0 |
   | --- | --- |
   | Base system: kernel source, systemd, glibc, mesa, PipeWire, NetworkManager, polkitd, bootc, SELinux, firmware | Upstream. Athanor's part is the build, the integration, the hardening and the signature, already its own. |
   | Session: everything the user sees, and everything that mediates input, output, authentication or privilege in the session. That is the compositor, greeter (UI and authentication path), lock, polkit agent and prompter, bar, dock, launcher and application library, overview and window switcher, OSD, notifications, control center, layout chooser, idle, wallpaper, the settings daemon's roles, output management, portal backend and recovery console | **Own.** |
   | First-party applications: the file manager with the file chooser (`athanor-files-view`), and Settings | **Own.** |
   | Other applications: terminal, text editor, disk utility, viewers | Third party: the RPM defaults of `doc_software.md` decision 7, or Flatpak. Athanor writes no disk utility of its own (`doc_software.md` section 9.1, `doc_disks.md` section 7): GNOME Disks stays the disk utility. Nautilus leaves in 1.0, replaced by Athanor's file manager, which amends `doc_software.md` section 9.2. |
   | The store | Third party: Bazaar for applications (`doc_software.md` section 9.4, option (b)), from the Fedora 45 base. Its Athanor page is own. `cosmic-store` leaves with decision 3. |
   | Compatibility bridges: xwayland-satellite | Upstream, as a library is. |
   | Assets: fonts, icon and cursor themes, and the GTK and Qt themes that third-party applications read | Upstream where their licence allows, but no COSMIC package (decision 3). |

3. **No COSMIC package is in the 1.0 image.**
   - cosmic-comp's session leaves the greeter in 1.0, and the fork leaves tier 2.
   - Sulphur is the only session, and it holds alone on the hardware matrix, NVIDIA included.
   - This amends ADR-0135 decision 3, which kept cosmic-comp at the greeter until 1.1, and the
     sentence of ADR-0121 decision 5 that put the GTK4 shell on top.
   - cosmic-icon-theme leaves with the rest. The shell's icons come from the Calmo visual language
     (`doc_visual_language.md`) or from a non-COSMIC theme.

4. **What returns to 1.0 from 1.1.** `doc_own_stack.md` decision 28 ties cosmic-comp's session
   leaving the greeter to OS19 in full, to the overview and to the window switcher. So these come
   back from ADR-0135 decision 3:
   - the overview, `athanor-overview`;
   - the retirement of cosmic-workspaces;
   - OS19 items 1 to 5 as written, including:
     - the trusted path tested in the OS6 fixture (`doc_session.md` SN16, gap 2);
     - the Orca keyboard monitor checked in the default session (gap 3);
     - the hour-long console run per machine.

   ADR-0138 already places the window switcher in 1.0.

5. **Every GTK4 program of Athanor is ported to Sal in 1.0:**
   - the dock, without its GTK feature;
   - the greeter UI, last, as `doc_own_stack.md` decision 34 orders it;
   - the recovery console.

   The sentence of `release-1.0.md` that keeps them GTK4 is amended. The file manager and Settings
   are written on Sal. `doc_files.md` and `doc_settings.md` take a revision that replaces
   libadwaita, before their code, under [ADR-0074](0074-agent-and-contributor-model.md).

6. **A session that fails falls back to Athanor's recovery console.** It used to fall back to
   cosmic-comp's session. `doc_session.md` revision 4's hand-off is revised to say so.

7. **"Own" is a check, not a statement.** One automatic check, in `verify.py` or `just check`,
   runs on the 1.0 build and on every pull request. It refuses:
   - in the image, any `cosmic-*` RPM and `pop-launcher`;
   - in the image, `gtk-layer-shell`, `gtk4-layer-shell` and `xdg-user-dirs-gtk`;
   - any `athanor-*` crate whose dependency tree holds `gtk4`, `libadwaita`, `libcosmic` or `iced`.

   It starts with an allow-list of today's entries above, each with its issue. Like
   `scripts/ci/known-red.txt`, the list only shrinks. 1.0 ships when it is empty.

8. **The order does not change.** The Fedora 45 rebase ships as release 0.10 before 2026-12-02,
   and comes first. After it, the work follows the order of the open plans:
   - the Sal revisions of `doc_files.md` and `doc_settings.md`;
   - the file chooser and the polkit agent (LP18), in parallel;
   - Settings;
   - then the rest of the allow-list.

## Alternatives rejected

- **1.0 on cosmic-comp, with Sulphur in 1.1.** Weighed on 2026-11-18. It leaves the privileged
  globals granted by an unauthenticated string (`com.system76.CosmicPanel`). It also leaves
  cosmic-comp's patches local, because pop-os refuses contributions written with a language model.
  ADR-0121 decision 5 had already rejected it.
- **1.0 on cosmic-comp, with Athanor's lock, polkit agent and greeter.** Weighed on 2026-11-18. It
  keeps the security work but not the compositor's: identity-based grants, the Orca keyboard
  monitor, the keyboard aids.
- **cosmic-comp at the greeter as a fallback until 1.1.** The maintainer answered no. The image
  would not be entirely own while it ships.
- **Every application own, the terminal, the viewers and the store included.** The maintainer
  answered no. Those stay third party in 1.0.

## Consequences

- 1.0 comes later than under ADR-0135. The work of decisions 4 and 5 is added to its path. 1.0
  still has no date, and this record gives no estimate.
- Sulphur alone carries the hardware matrix, so #604 (the NVIDIA desktop's GPU off the bus on
  resume) can block it.
- Documents to revise, each under ADR-0074:
  - `release-1.0.md`, its 1.0 paragraph and section 3;
  - `doc_session.md`, SN16 and revision 4's hand-off;
  - `doc_own_stack.md`, decision 28;
  - `doc_launcher.md`, LA12;
  - `doc_shell.md`;
  - `doc_files.md`;
  - `doc_settings.md`;
  - `doc_software.md`: decision 7, section 9.2 and SW15 to SW17;
  - `doc_lock_and_prompts.md` and `doc_session_daemons.md`: cosmic-greeter as the locker and
    cosmic-idle;
  - `doc_visual_language.md`: the rule that applications keep libadwaita;
  - the libadwaita mentions of the documents listed in the Context;
  - `docs/architecture/components.toml`: each COSMIC component's entry.
- Amended:
  - [ADR-0121](0121-own-compositor-and-toolkit.md) decision 5: its words "with the GTK4 shell on
    top";
  - [ADR-0135](0135-faster-road-to-1-0.md) decision 3: cosmic-comp's session at the greeter in
    1.0, and its move to 1.1 of the overview, the retirement of cosmic-workspaces and OS19 in full;
  - [ADR-0138](0138-own-components-in-1-0.md) decision 2: its sentence that the overview and
    cosmic-workspaces' retirement stay in 1.1;
  - [ADR-0122](0122-own-compositor-optional-session-and-desktop-first.md) and
    [ADR-0132](0132-own-compositor-default-before-full-acceptance.md): cosmic-comp's session as
    the second choice at the greeter;
  - [ADR-0103](0103-audit-5-decisions.md) D13: cosmic-workspaces kept until 1.1.
- The check of decision 7 is opened as an issue and lands before the first port that it would
  measure.

## Maintainer's answers

Given on 2026-11-18 (translated from Italian):

1. Does "everything own" in 1.0 cover first-party applications? "Session plus Files and
   Settings". The other applications stay third party.
2. Does cosmic-comp stay at the greeter as a fallback in 1.0? "No, it leaves in 1.0".
3. Do the overview and the retirement of cosmic-workspaces return to 1.0? "Yes, in 1.0".
