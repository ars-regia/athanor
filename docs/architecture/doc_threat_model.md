# Athanor threat model

Status: **revision 1, 2026-10-06,** written from maintainer decision A2-9 (#151) of 2026-10-05, with maintainer decisions A2-6 (#151) and A2-7 (#151) for the trusted path and the keyring prompter. Source: the second specification audit, security and trust group, findings 2 and 4. It is the one threat model of the project: every specification states its guarantees against the tiers defined here and cites this document instead of writing an adversary of its own.

## 1. Why one document

Before this revision each specification wrote its own adversary paragraph, and they disagreed on the one question that decides most designs: whether code running as the user is an adversary. `doc_lock_and_prompts.md` (section 1), `doc_bar.md` BR1 and `doc_accessibility.md` (the reader gate) said no; LP13, LP8's cgroup admission and SD8's `confined` class built defences that only hold if it is. SD9 recorded the bridge between the two: a confined application could write the user's autostart entries and user units and run unconfined at the next login. This document settles the question once, in three tiers.

## 2. The three tiers

**TM1. Tier 1: unconfined user code is the user.** A process that runs as the user outside any confinement holds the user's rights, and Athanor does not defend the user's session against it, as GNOME and KDE do not.

- **Members.** The user's commands in a terminal and everything they start; the `unconfined` class of `doc_session_daemons.md` SD8 (Ptyxis, Software, system autostart entries); Nix tools, which carry the "not confined" badge (`doc_software.md` SW10, maintainer decision A2-16 (#155)); user units written by hand; `toolbox` containers, which are not isolation (TM5).
- **What it can do, stated once.** Kill the lock, call logind, replace or extend any user unit with a drop-in, rewrite dconf, read every file of the user, imitate any prompt on the main Wayland socket, and lock the administrator out for five minutes through faillock (`doc_lock_and_prompts.md` D3).
- **Consequence for every specification.** A check that identifies a peer by its unit or cgroup (`doc_bar.md` BR1, `doc_lock_and_prompts.md` LP8, `doc_accessibility.md`'s reader gate, `doc_first_run.md` FR6) is an integrity check against mistakes and against tier 2. It is never described as a barrier against tier 1. A specification that needs protection against tier 1 is asking for a filesystem sandbox of the whole session, which no decision has taken.

**TM2. Tier 2: confined applications are untrusted.** Flatpak applications and applications of the broker's `confined` class (`doc_session_daemons.md` SD8) are treated as hostile. Everything that must hold against them is listed here; a specification that relies on a further property adds it to this list.

- **No bus name outside the filter:** the broker's `xdg-dbus-proxy` (SD9) or Flatpak's own proxy.
- **No main Wayland socket:** a security-context socket only (`doc_bar.md` BR2, SD8), so no privileged global, and its floating windows carry the compositor's border once LP13 ships (TM6).
- **No unit manager:** `$XDG_RUNTIME_DIR/systemd/` is hidden and `org.freedesktop.systemd1` is not on the filter (SD8, SD9).
- **No new privileges:** `NoNewPrivileges=yes` (SD8); Flatpak sets it for its own sandbox.
- **No write to the persistence paths of TM3.**

**TM3. Persistence paths.** A path is a persistence path when what is written there runs, or changes what runs, at a later login or a later command of the user without a new action of the user. Tier 2 is denied write on the paths listed below; the list names the known persistence paths, not every path that meets the definition (candidates in T2). For the broker's `confined` class the rule is enforced with `ReadOnlyPaths=` on the application's unit (SD8); for Flatpak it is Flatpak's own sandbox, within the limits of TM4.

| Path                                                          | Why it is a persistence path                                                                                |
| ------------------------------------------------------------- | ----------------------------------------------------------------------------------------------------------- |
| `~/.config/systemd`                                           | user units and drop-ins, including drop-ins on the lock, the agent and the bar                              |
| `~/.local/share/systemd`                                      | the user manager's second unit directory (`$XDG_DATA_HOME/systemd/user`); A2-29 (#151)                      |
| `~/.config/autostart`                                         | XDG autostart entries, started at every login (SD10)                                                        |
| `~/.config/environment.d`                                     | environment of the user manager, so of every unit (`LD_PRELOAD`, `PATH`)                                    |
| `~/.bash_profile`, `~/.bash_login`, `~/.profile`, `~/.bashrc` | shell start-up files                                                                                        |
| `~/.bashrc.d`                                                 | the image's `/etc/skel/.bashrc` sources every file in it; A2-29 (#151)                                      |
| `~/.local/share/applications`                                 | desktop entries the launcher and `xdg-open` run                                                             |
| `~/.local/share/nautilus/scripts`                             | scripts the file manager offers to run                                                                      |
| `~/.config/cosmic`                                            | the compositor's configuration, whose shortcuts run commands; A2-29 (#151)                                  |
| `~/.local/bin`, `~/bin`                                       | first on `PATH` in the image's `~/.bashrc`: a file there replaces a command the user types; A2-29 (#151)    |
| `~/.local/share/dbus-1/services`                              | the session bus's user activation directory; A2-29 (#151)                                                   |
| `~/.local/share/flatpak/overrides`                            | Flatpak's user overrides, which widen another application's permissions; A2-29 (#151)                       |
| `~/.gitconfig`                                                | `core.hooksPath` and `core.fsmonitor` run a program at the user's next git command; A2-29 (#151)            |
| `~/.ssh/config`                                               | `ProxyCommand` runs at the user's next `ssh`; A2-29 (#151)                                                  |
| `~/.config/mimeapps.list`                                     | chooses the program `xdg-open` and the file manager run for a file type; A2-29 (#151)                       |

- **Rows beyond the issue's list.** `~/.local/share/systemd`, `~/.bashrc.d` and `~/.config/cosmic`, and the candidates of former open doubt T2, are not in the list of issue #151. The maintainer added them on 2026-10-06 (A2-29 (#151)); SD8's `ReadOnlyPaths=` (`doc_session_daemons.md`) covers them.
- **The list is a deny-list and needs upkeep.** A new program that runs code from a file in the home adds its path here, in the same change that ships the program. This is the accepted cost of not building a filesystem sandbox (SD21).
- **A path must exist to be protected.** `ReadOnlyPaths=` binds only paths present when the unit starts, and the `-` prefix that tolerates an absent path leaves it creatable by the application. The broker creates the listed directories before each launch; an absent start-up file (`~/.bash_login`, `~/.profile`, `~/.gitconfig`, `~/.ssh/config`, `~/.config/mimeapps.list` and the like) is protected by a bind of an empty read-only file over its path in the unit, so the application can neither create it nor write it, and nothing is created in the home (A2-29 (#151)).
- **Stated residual risk:** dconf. Every GTK application that stores a preference needs `ca.desrt.dconf`, and dconf has no per-key access control, so a confined application can change any user setting, among them `org.athanor.desktop.idle` (SD9).

**TM4. Where tier 2 leaks, stated.** A Flatpak application granted home or host filesystem access can write the persistence paths and leave its sandbox (`doc_kernel_profile.md` section 10, residual risks); Software shows that permission (`doc_software.md` SW3). An RPM application in the `unconfined` class is tier 1 by policy. The default browser moves to Flatpak (maintainer decision A2-15 (#159)), which takes the program most exposed to hostile input out of the RPM class.

**TM5. toolbox is not isolation.** `toolbox` creates its containers privileged, in the host's process and network namespaces, with SELinux separation disabled and the host's root mounted at `/run/host` (the `toolbox` 0.3 binary of the image carries `--privileged`, `--pid`, `--network`, `label=disable` and `/:/run/host:rslave`, read on the maintainer's desktop on 2026-10-06). Code in a toolbox is tier 1. The isolating paths for untrusted tools are a rootless podman container with SELinux `container_t` and none of those options (`doc_software.md` SW8), or the dev VM.

**TM6. Trusted path and prompts.** The guarantees of `doc_lock_and_prompts.md` LP13 (maintainer decision A2-6 (#151)) and LP12 (maintainer decision A2-7 (#151)) hold against tier 2 at most. A confined application connects through a security context. cosmic-comp PR #1441, open and not merged (read 2026-10-06), draws an optional border, enabled by rules on the security context's sandbox engine and app id and drawn in a configured colour, around the floating windows of matching clients; with the rule LP13 sets, a confined application's floating window that imitates a prompt carries the border and the real prompt does not. Fullscreen and tiled windows carry no border, so a fullscreen imitation is not told apart (T4). A tier 1 process can connect to the main socket and is not covered.

**TM7. Tier 3: root and the image** belong to `doc_kernel_profile.md` (sections 8 to 10: image signatures, Secure Boot and the MOK chain, module signing, SELinux, the root boundary and its residual risks). This document does not restate them. Attestation (`system/confidential_computing/athanor-attestation`) is not part of any tier at 1.0. On 2026-10-07 the maintainer decided that the crate leaves the workspace (PR #280) and is rewritten on Keylime with the PCR 11 and PCR 12 policy of `doc_kernel_profile.md` section 9 before mesh admission depends on it; it is kept, not deleted.

## 3. The rule for services

**TM8. Every service sets `NoNewPrivileges=yes` or a capability allow-list.** It replaces the project rule "no daemon or application outside a compartment or a MicroVM", which named a mechanism no specification defined and no code implemented (audit 2, security and trust, finding 4).

- **Scope.** Every systemd service unit the repository ships: unit files, units written by a package specification, and the repository's drop-ins on those units, from every directory systemd reads for them (systemd.unit(5)): `service.d`, each dash prefix (`foo-.service.d` for `foo-bar.service`), the template's (`foo@.service.d` for `foo@x.service`) and the unit's own. A drop-in directory with no unit of the repository (an upstream unit's, a prefix, a template or `service.d`) is checked on its own when it adds any command (`ExecStart=`, `ExecStartPre=` or another `Exec*=` setting), because that command is ours. An empty unit file is a mask and runs nothing.
- **What counts.** The merged `[Service]` section, unit then drop-ins in file-name order (a file in a more specific directory replacing a same-named one), ends with `NoNewPrivileges=` true, or with a `CapabilityBoundingSet=` that is an allow-list: a positive list, or an empty assignment that drops every capability. A deny-list (`~CAP_SYS_MODULE ...`) does not count, even one that removes `CAP_SYS_ADMIN`: it keeps every capability it does not name, and several of them (`CAP_SYS_ADMIN`, `CAP_SYS_MODULE`, `CAP_DAC_OVERRIDE`, `CAP_SYS_PTRACE`) each give root back. A unit that needs a deny-list also sets `NoNewPrivileges=yes`, as the `athanor-update` units do. The setting is written explicitly: systemd also implies `NoNewPrivileges` from options such as `DynamicUser=` or `SystemCallFilter=`, but the rule asks for the setting a reader and the check can see.
- **Enforcement.** `python3 scripts/verify.py services`, run in CI by the lint workflow. A unit that cannot meet the rule is named in the check's exemption list with its reason, so every exemption is visible; an exemption that no longer applies fails the check.
- **What does not change.** Shell programs confine themselves with Landlock (`doc_kernel_profile.md` section 10); third-party applications are Flatpak by default; MicroVMs are only for received workloads after 1.0 (`doc_kernel_profile.md` D28).
- **The project instructions** carry the rule in the text the maintainer approved on 2026-10-07; it lands through a separate change, until which the instructions still hold the earlier rule.

## 4. The polkit model

**TM9. Every polkit action Athanor declares or overrides, and its result for the active session.** The table has one row for each action that a `.policy` file of the repository declares and for each action that a polkit `.rules` file of the repository overrides. Every other action keeps the default its upstream package declares. "Active session" is the result for a process of the user's active local session: for a `.policy`, its `allow_active`; for a rule, every result the rule returns. `auth_admin` asks for the password of an administrator. The image's `10-athanor-wheel-admin.rules` makes the members of `wheel` the only administrators (`polkit.addAdminRule`). Since 2026-10-07 the image overrides no upstream action: the udisks2 mount and eject rule went with `doc_disks.md` DK20 A, and a rule for `org.containers.bootc.status`, an action no package of the image declares, was removed (#282).

| Action | Declared in | Active session | Shipped | Notes |
| --- | --- | --- | --- | --- |
| `os.athanor.update.apply` | `forge/specs/athanor-update/SOURCES/usr/share/polkit-1/actions/os.athanor.update.policy` | `yes` | yes | Applies an update the service has already verified (`doc_update_trust.md`). Inactive and remote sessions: `auth_admin_keep`. |
| `os.athanor.update.rollback` | `forge/specs/athanor-update/SOURCES/usr/share/polkit-1/actions/os.athanor.update.policy` | `auth_admin` | yes | Returns to the previous deployment. |
| `org.freedesktop.login1.inhibit-block-sleep` | `scripts/runner/50-athanor-runner-inhibit.rules` | `yes` | no | Installed on the CI runner host only, for the user `athanor-runner`, which runs outside any session. |
| `os.athanor.cloudsync.mount` | `forge/specs/athanor-cloud-rs/athanor-cloud-rs-1.0.0/os.athanor.cloud.policy` | `auth_admin_keep` | no | Package out of the image. |
| `os.athanor.lvfs.apply` | `forge/specs/athanor-lvfs-rs/athanor-lvfs-rs-1.0.0/os.athanor.lvfs.policy` | `auth_admin_keep` | no | Package out of the image. |
| `os.athanor.mdm.wipe` | `forge/specs/athanor-mdm-rs/athanor-mdm-rs-1.0.0/os.athanor.mdm.policy` | `auth_admin` | no | Package out of the image. |
| `os.athanor.mdm.apply_policy` | `forge/specs/athanor-mdm-rs/athanor-mdm-rs-1.0.0/os.athanor.mdm.policy` | `auth_admin` | no | Package out of the image. |
| `os.athanor.store.install` | `forge/specs/athanor-store-rs/athanor-store-rs-1.0.0/os.athanor.store.policy` | `auth_admin_keep` | no | Package out of the image since 2026-09-17. |

- **Enforcement.** `python3 scripts/verify.py polkit-model`, run in CI by the lint workflow, fails in three cases: an action declared or overridden in a tracked `.policy` or polkit `.rules` file has no row; a row names an action its file does not declare; or the "Active session" column differs from the file. A rule the check cannot read also fails, so a new rule cannot escape the table: the check reads one form only, after dropping comments and setting string literals aside. A file holds nothing but `polkit.addRule(...)` and `polkit.addAdminRule(...)` calls. Each `addRule` function is `function(action, subject) { if (<test>) { ... } }` with nothing after the block, where `<test>` is `action.id == "literal"`, or several joined by `||`, optionally in parentheses and followed by `&&` conditions that do not name the action; the block returns only `polkit.Result.X` and does not name the action. Anything else fails: a negated test, a result in an `else`, an alias, `action["id"]`, a prefix match, a concatenated id, a template literal, `eval`. Every action of the test gets every result of the block. Only the active session is checked: `allow_any` and `allow_inactive` of a `.policy`, and what a rule returns outside the active session, are stated in "Notes" and reviewed by hand, as are the "Shipped" and "Notes" columns.
- **Who answers for a row.** The change that adds or edits a `.policy` or polkit `.rules` file edits its row in the same change. A result that grants more than upstream (`yes` where upstream asks for authentication) states its reason in "Notes".
- **The polkit check** (`verify.py polkit`) is a different check. It fails when Athanor code enforces an action that no `.policy` declares.

## 5. How specifications cite this document

A specification's threat paragraph names the tier each of its guarantees holds against and cites the TM rule; it adds only what is specific to it. Amended on 2026-10-06: `doc_lock_and_prompts.md`, `doc_software.md`, `doc_session_daemons.md`, `doc_kernel_profile.md`.

Owed at each document's next revision: `doc_shell.md` SH12 and `doc_bar.md` BR1 (the informative checks are TM1's integrity checks), `doc_accessibility.md` (the reader gate's residual risk is TM1), `doc_portal.md` and `doc_files.md` (the persistence paths of TM3 for the file manager and the portal's FileChooser).

## 6. Open doubts

1. **T1. Absent start-up files.** Decided 2026-10-06 (A2-29 (#151)): the unit binds an empty read-only file over each absent path (TM3).
2. **T2. Further candidates for TM3.** Decided 2026-10-06 (A2-29 (#151)): all of them are rows of TM3.
3. **T3. The polkit model.** Which actions Athanor adds or overrides and their result for the active session have no single owner (audit 2, security and trust, finding 15). Decided 2026-10-07 by the maintainer: the table belongs in this document, with a check (TM9).
4. **T4. Windows the trusted-path border does not mark.** cosmic-comp#1441 borders floating windows only, so a confined application can imitate a prompt with a fullscreen or tiled window (TM6). Decided 2026-10-06 (A2-31): the extension of the indicator to every window state is proposed upstream with #1441; until it lands, the gap is a stated residual risk of TM6 (`doc_lock_and_prompts.md` L13).

## 7. Acceptance

1. `python3 scripts/verify.py services` and `python3 scripts/verify.py polkit-model` pass in CI.
2. On the dev VM, a `confined` test application fails to create or change a file in each path of TM3, and the same write succeeds from a terminal (SD22 step 6).
