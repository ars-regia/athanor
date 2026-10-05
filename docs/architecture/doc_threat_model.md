# Athanor threat model

Status: **revision 1, 2026-10-06,** written from the maintainer's decision A2-9 of 2026-10-05 (issue #151), with A2-6 and A2-7 (same issue) for the trusted path and the keyring prompter. Source: the second specification audit, security and trust group, findings 2 and 4. It is the one threat model of the project: every specification states its guarantees against the tiers defined here and cites this document instead of writing an adversary of its own.

## 1. Why one document

Before this revision each specification wrote its own adversary paragraph, and they disagreed on the one question that decides most designs: whether code running as the user is an adversary. `doc_lock_and_prompts.md` (section 1), `doc_bar.md` BR1 and `doc_accessibility.md` (the reader gate) said no; LP13, LP8's cgroup admission and SD8's `confined` class built defences that only hold if it is. SD9 recorded the bridge between the two: a confined application could write the user's autostart entries and user units and run unconfined at the next login. This document settles the question once, in three tiers.

## 2. The three tiers

**TM1. Tier 1: unconfined user code is the user.** A process that runs as the user outside any confinement holds the user's rights, and Athanor does not defend the user's session against it, as GNOME and KDE do not.

- **Members.** The user's commands in a terminal and everything they start; the `unconfined` class of `doc_session_daemons.md` SD8 (Ptyxis, Software, system autostart entries); Nix tools, which carry the "not confined" badge (`doc_software.md` SW10, decision A2-16, issue #155); user units written by hand; `toolbox` containers, which are not isolation (TM5).
- **What it can do, stated once.** Kill the lock, call logind, replace or extend any user unit with a drop-in, rewrite dconf, read every file of the user, imitate any prompt on the main Wayland socket, and lock the administrator out for five minutes through faillock (`doc_lock_and_prompts.md` D3).
- **Consequence for every specification.** A check that identifies a peer by its unit or cgroup (`doc_bar.md` BR1, `doc_lock_and_prompts.md` LP8, `doc_accessibility.md`'s reader gate, `doc_first_run.md` FR6) is an integrity check against mistakes and against tier 2. It is never described as a barrier against tier 1. A specification that needs protection against tier 1 is asking for a filesystem sandbox of the whole session, which no decision has taken.

**TM2. Tier 2: confined applications are untrusted.** Flatpak applications and applications of the broker's `confined` class (`doc_session_daemons.md` SD8) are treated as hostile. Everything that must hold against them is listed here; a specification that relies on a further property adds it to this list.

- **No bus name outside the filter:** the broker's `xdg-dbus-proxy` (SD9) or Flatpak's own proxy.
- **No main Wayland socket:** a security-context socket only (`doc_bar.md` BR2, SD8), so no privileged global and no unmarked surface (TM6).
- **No unit manager:** `$XDG_RUNTIME_DIR/systemd/` is hidden and `org.freedesktop.systemd1` is not on the filter (SD8, SD9).
- **No new privileges:** `NoNewPrivileges=yes` (SD8); Flatpak sets it for its own sandbox.
- **No write to the persistence paths of TM3.**

**TM3. Persistence paths.** A path is a persistence path when what is written there runs, or changes what runs, at a later login or a later command of the user without a new action of the user. A confined application never writes one. For the broker's `confined` class the rule is enforced with `ReadOnlyPaths=` on the application's unit (SD8); for Flatpak it is Flatpak's own sandbox, within the limits of TM4.

| Path                                                                         | Why it is a persistence path                                                              |
| ---------------------------------------------------------------------------- | ----------------------------------------------------------------------------------------- |
| `~/.config/systemd`                                                          | user units and drop-ins, including drop-ins on the lock, the agent and the bar            |
| `~/.local/share/systemd`                                                     | the user manager's second unit directory (`$XDG_DATA_HOME/systemd/user`)                  |
| `~/.config/autostart`                                                        | XDG autostart entries, started at every login (SD10)                                      |
| `~/.config/environment.d`                                                    | environment of the user manager, so of every unit (`LD_PRELOAD`, `PATH`)                  |
| `~/.bash_profile`, `~/.bash_login`, `~/.profile`, `~/.bashrc`, `~/.bashrc.d` | shell start-up files; the image's `/etc/skel/.bashrc` sources every file of `~/.bashrc.d` |
| `~/.local/share/applications`                                                | desktop entries the launcher and `xdg-open` run                                           |
| `~/.local/share/nautilus/scripts`                                            | scripts the file manager offers to run                                                    |
| `~/.config/cosmic`                                                           | the compositor's configuration, whose shortcuts run commands                              |

- **The list is a deny-list and needs upkeep.** A new program that runs code from a file in the home adds its path here, in the same change that ships the program. This is the accepted cost of not building a filesystem sandbox (SD21).
- **A path must exist to be protected.** `ReadOnlyPaths=` binds only paths present when the unit starts, and the `-` prefix that tolerates an absent path leaves it creatable by the application. The broker creates the listed directories before each launch; how an absent start-up file is protected is open doubt T1.
- **Stated residual risk:** dconf. Every GTK application that stores a preference needs `ca.desrt.dconf`, and dconf has no per-key access control, so a confined application can change any user setting, among them `org.athanor.desktop.idle` (SD9).

**TM4. Where tier 2 leaks, stated.** A Flatpak application granted home or host filesystem access can write the persistence paths and leave its sandbox (`doc_kernel_profile.md` section 10, residual risks); Software shows that permission (`doc_software.md` SW3). An RPM application in the `unconfined` class is tier 1 by policy. The default browser moves to Flatpak (decision A2-15, issue #159), which takes the program most exposed to hostile input out of the RPM class.

**TM5. toolbox is not isolation.** `toolbox` creates its containers privileged, in the host's process and network namespaces, with SELinux separation disabled and the host's root mounted at `/run/host` (the `toolbox` 0.3 binary of the image carries `--privileged`, `--pid`, `--network`, `label=disable` and `/:/run/host:rslave`, read on the maintainer's desktop on 2026-10-06). Code in a toolbox is tier 1. The isolating paths for untrusted tools are a rootless podman container with SELinux `container_t` and none of those options (`doc_software.md` SW8), or the dev VM.

**TM6. Trusted path and prompts.** The guarantees of `doc_lock_and_prompts.md` LP13 (decision A2-6) and LP12 (decision A2-7) hold against tier 2 only. A confined application connects through a security context, and once LP13 ships the compositor marks every security-context client (cosmic-comp PR #1441), so the application cannot present an unmarked surface that imitates a prompt. A tier 1 process can connect to the main socket and is not covered.

**TM7. Tier 3: root and the image** belong to `doc_kernel_profile.md` (sections 8 to 10: image signatures, Secure Boot and the MOK chain, module signing, SELinux, the root boundary and its residual risks). This document does not restate them. Attestation (`system/confidential_computing/athanor-attestation`) is not part of any tier at 1.0: the crate is to move to `experimental/` pending the maintainer's decision, because it is a protected path (`doc_kernel_profile.md` section 9).

## 3. The rule for services

**TM8. Every service sets `NoNewPrivileges=yes` or a capability bound.** It replaces the project rule "no daemon or application outside a compartment or a MicroVM", which named a mechanism no specification defined and no code implemented (audit 2, security and trust, finding 4).

- **Scope.** Every systemd service unit the repository ships: unit files, units written by a package specification, and the repository's drop-ins on those units. A drop-in on an upstream unit is checked only if it replaces the unit's `ExecStart=`. An empty unit file is a mask and runs nothing.
- **What counts.** The merged `[Service]` section, unit then drop-ins in name order, ends with `NoNewPrivileges=` true, or assigns `CapabilityBoundingSet=` (an empty assignment drops every capability). The setting is written explicitly: systemd also implies `NoNewPrivileges` from options such as `DynamicUser=` or `SystemCallFilter=`, but the rule asks for the setting a reader and the check can see.
- **Enforcement.** `python3 scripts/verify.py services`, run in CI by the lint workflow. A unit that cannot meet the rule is named in the check's exemption list with its reason, so every exemption is visible; an exemption that no longer applies fails the check.
- **What does not change.** Shell programs confine themselves with Landlock (`doc_kernel_profile.md` section 10); third-party applications are Flatpak by default; MicroVMs are only for received workloads after 1.0 (`doc_kernel_profile.md` D28).
- **`CLAUDE.md`** is the maintainer's file: its "Limiti inviolabili" rule changes only after the maintainer reviews the replacement text.

## 4. How specifications cite this document

A specification's threat paragraph names the tier each of its guarantees holds against and cites the TM rule; it adds only what is specific to it. Amended on 2026-10-06: `doc_lock_and_prompts.md`, `doc_software.md`, `doc_session_daemons.md`, `doc_kernel_profile.md`.

Owed at each document's next revision: `doc_shell.md` SH12 and `doc_bar.md` BR1 (the informative checks are TM1's integrity checks), `doc_accessibility.md` (the reader gate's residual risk is TM1), `doc_portal.md` and `doc_files.md` (the persistence paths of TM3 for the file manager and the portal's FileChooser).

## 5. Open doubts

1. **T1. Absent start-up files.** `ReadOnlyPaths=` cannot protect a file that does not exist; bash reads the first of `~/.bash_profile`, `~/.bash_login` and `~/.profile` that exists, and the image's skeleton ships only `~/.bash_profile`. Options: the broker creates empty files, or the unit binds an empty read-only file over each absent path. Decided with SD22 step 6.
2. **T2. Further candidates for TM3,** not yet decided: `~/.local/bin` and `~/bin`, which the image's `~/.bashrc` puts first on `PATH`, so a file there replaces a command the user types; `~/.local/share/dbus-1/services`, the session bus's user activation directory; Flatpak's user overrides under `~/.local/share/flatpak/overrides`, which widen another application's permissions.
3. **T3. The polkit model.** Which actions Athanor adds or overrides and their result for the active session have no single owner (audit 2, security and trust, finding 15). Whether that table belongs here is for the maintainer.

## 6. Acceptance

1. `python3 scripts/verify.py services` passes in CI.
2. On the dev VM, a `confined` test application fails to create or change a file in each path of TM3, and the same write succeeds from a terminal (SD22 step 6).
