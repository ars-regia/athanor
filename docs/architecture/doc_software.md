# Athanor Software: applications, background activity and developer tools

Status: Approved, rev 2 (2026-09-30): the maintainer accepted every recommendation of section 6, which now records the decisions. Revision 3 (2026-10-05) adds decision 7, the default applications, and amends decision 6 accordingly; it awaits the maintainer's review. Amended 2026-10-06 (maintainer decision A2-9 (#151)): the project rule and the confinement of user workloads follow `doc_threat_model.md`, and decision 3 (c) no longer counts `toolbox` as isolation (ADR-0086). Section 9 (2026-10-06) records maintainer decisions A2-14, A2-15 and A2-24 (#159): GNOME Disks, Nautilus until 1.0, Firefox as a Flatpak, the written comparison with Bazaar that this document needs before approval, and the format of the offline help, which the maintainer chose (A2-30, #159); it overrides the text above where they disagree. It turns the maintainer's request of 2026-09-30 into a specification: one application, working title **Software**, with which an average user never has to struggle and which also serves developers. Section 6 records the maintainer's decisions, with the options that were weighed. Section 4 lists what must be proven before the first plan is written.

## 1. Context

### 1.1 What binds this document

- `doc_shell.md`: the replacement rule and no facades (SH1, `doc_shell.md:36-42`); COSMIC applications, `cosmic-store` among them, are content and not dependencies (SH3, `doc_shell.md:73`; for the applications, superseded by decision 7); one crate per program on plain `gtk4-rs` (SH4, `doc_shell.md:75-76`); the Calmo tokens, WCAG AA contrast, high contrast and reduced motion (SH5, `doc_shell.md:84-96`); every string read from a system file is plain text, truncated, stripped of control and bidirectional characters (SH12, `doc_shell.md:149`); the test matrix, AT-SPI roles and names, gettext from the first commit (SH13, `doc_shell.md:172-182`).
- `doc_bar.md`: applications start behind a `wp_security_context_v1` socket in a transient unit of the user manager (BR2, `doc_bar.md:34-53`); untrusted strings are plain text (BR4, `doc_bar.md:79`); the shield and its sheet (BR6, `doc_bar.md:105-119`).
- `doc_update_trust.md`: system image updates, the state file and the two requests (UT6, UT7, UT11). **This document does not touch system image updates.** They stay in the shield and its sheet (`doc_bar.md`, BR3 and BR6) and in the notifier (`doc_update_trust.md`, UT11). Software shows the running version read-only and points at the shield (SW16).
- `doc_kernel_profile.md`: applications update through Flatpak with no interruption (class A, `doc_kernel_profile.md:479`); on the desktop class code in the home runs and is measured, and a quarantine prompt outside the kernel is bypassable, a stated residual risk (D23, `doc_kernel_profile.md:100`); code running as the user persists through autostart entries and `systemd --user` units, and a Flatpak application with home access can leave its sandbox (`doc_kernel_profile.md:715-722`).
- The threat model is `doc_threat_model.md` (amended 2026-10-06, maintainer decision A2-9 (#151)). It replaces the project rule "no daemon or application outside a compartment or a MicroVM": code running as the user outside confinement is the user (TM1), confined applications are untrusted (TM2), and every shipped service sets `NoNewPrivileges=yes` or a capability bound (TM8). SW10 states which tier each workload is in.

### 1.2 What ships today

Checked in the repository at `27378de3`, and, where the repository cannot answer, on the maintainer's desktop on 2026-09-30. That desktop runs an Athanor image older than `iso-v0`: it carries no `os.athanor.update.policy`, so desktop findings may lag the branch.

**Applications.**

- `flatpak` is installed by the image build (`system/Containerfile:143`, `forge/config/packages.json:113`), on the base `quay.io/fedora-ostree-desktops/base-atomic:43` (`system/Containerfile:41`).
- `cosmic-store` ships (`forge/config/packages.json:156`) and is today the only graphical way to install an application.
- **No Flathub remote is configured.** The only remote on the desktop is `fedora` (system, OCI), which Fedora's `flatpak-add-fedora-repos.service` adds (enabled by Fedora's `90-default.preset:374`, desktop). The one script that adds Flathub and installs the `flatpaks` list of `forge/config/packages.json:203-208` is `system/scripts/provision_flatpak.sh:24-36`; the image copies it to `/scripts/` (`system/Containerfile:145`), and no unit, kickstart or script runs it. That list therefore installs nothing, as the `upstream_*` lists once did.
- The system helper's polkit defaults (Flatpak 1.16.6, desktop): install and uninstall are `auth_admin_keep` for an active session, update and AppStream refresh are `yes` for an active session and `auth_admin` otherwise. Fedora's `org.freedesktop.Flatpak.rules` lets an active local member of `wheel` install and uninstall without a password.

**The legacy store crates.**

- `forge/specs/athanor-store-rs` is out of the workspace (`Cargo.toml:55`, `exclude`) and out of the image (absent from `forge/config/packages.json:2-48`). `experimental/EXEMPT:18-22` records why: it ran `flatpak install` for any D-Bus caller without the polkit check its policy declares, and verified signatures against a caller-supplied key. Read on 2026-09-30:
  - The unit is a system unit with `Type=dbus`, `BusName=os.athanor.Store` and `DynamicUser=yes` (`athanor-store-rs.spec:50-60`), the bus policy lets only root own the name (`os.athanor.Store.conf:5-7`), and the code connects to the **session** bus (`src/backend/dbus.rs:105`). It could never have started.
  - `os.athanor.store.install` is declared (`os.athanor.store.policy:9`) and checked nowhere.
  - `verify_pqc_package` verifies a Dilithium signature against a public key the caller passes in (`src/backend/dbus.rs:31-51`), which proves nothing: a facade in a security path.
  - The catalog is `flatpak list` of installed applications with a constant rating of 5.0 (`src/backend/repository.rs:54-88`); `install_app` in `src/backend/flatpak.rs:15` is called by nothing.
- `system/athanor-store` is a second crate, a workspace member (`Cargo.toml:39`) that no spec builds. Its `install` command pins the "verified" digest to the literal `@sha256:PINNED_IMMUTABLE_HASH_PLACEHOLDER` (`system/athanor-store/src/main.rs:193`), and it can delete the Flathub remote (`main.rs:86-95`). Its storage engine is referenced by name from the exempt mesh crates.

**Nix.**

- `nix` and `nix-daemon` are in `upstream_core` (`forge/config/packages.json:104-105`); the `nix` meta-package does not pull the daemon.
- `/nix` is a bind mount of `/var/nix` (`forge/specs/athanor-nix-support/SOURCES/usr/lib/systemd/system/nix.mount:26-30`), ordered after `systemd-tmpfiles-setup` with `DefaultDependencies=no` (`nix.mount:18-24`), with the store skeleton seeded by tmpfiles (`usr/lib/tmpfiles.d/10-athanor-nix.conf:15-21`) and the daemon socket directory by `etc/tmpfiles.d/nix-daemon.conf:9-10`.
- `nix.mount` and `nix-daemon.socket` are enabled by preset (`80-athanor-nix.preset:13-14`; `nix-daemon.socket` again in `80-athanor-system.preset:7`). An SELinux module lets PID 1 create the daemon socket on the `default_t` label of `/nix` (`forge/specs/athanor-selinux/SOURCES/athanor_nix_daemon.te:9-18`); Fedora's policy has no Nix type.
- Users are multi-user Nix clients through the daemon. `/etc/nix/nix.conf` enables `nix-command flakes` and sets `nixpkgs=flake:nixpkgs`, so `nixpkgs` resolves through the flake registry over the network (desktop).

**Podman and Quadlet.**

- `podman` 5.8.4 and `toolbox` are on the desktop, from the base image: neither is in `forge/config/packages.json`. Quadlet's generators are installed for both managers: `/usr/lib/systemd/system-generators/podman-system-generator` and `/usr/lib/systemd/user-generators/podman-user-generator`, both links to `/usr/libexec/podman/quadlet` (desktop). The maintainer already runs three user Quadlet containers.
- The signature policy accepts any image outside the three system repositories: `transports.docker[""]` is `insecureAcceptAnything` (`doc_update_trust.md:52`).
- User namespaces are available to rootless podman (`doc_kernel_profile.md:704-706`; `kernel.unprivileged_userns_clone = 1` in `forge/specs/athanor-system-tweaks/SOURCES/usr/lib/sysctl.d/99-bore.conf:20`). Unprivileged ports start at 1024 (desktop).

**Services and presets.**

- Our presets are numbered below 81 so that they win over Fedora's (`80-athanor-base.preset:1-9`). `80-athanor-system.preset:5-12` enables greetd, firewalld, the Nix socket, `fstrim.timer`, `athanor-timewarp.timer`, `athanor-backup-hourly.timer`, usbguard and boltd.
- Fedora's `90-default.preset` (desktop) enables, among others, `sshd.service` (line 25), `ModemManager.service` (39), `bluetooth.*` (51), `avahi-daemon.*` (54), `cups.socket` and `cups.path` (59-60) and `pcscd.socket` (227). The install kickstart enables `sshd` and opens only SSH in the firewall (`system/athanor-install.ks:37-38`); password login is off (`forge/specs/athanor-base-config/SOURCES/etc/ssh/sshd_config.d/50-athanor-security.conf`).
- `samba` and a remote-desktop server are not installed; `wsdd` is installed and not enabled (desktop).
- `athanor-backup-hourly.service` calls `org.athanor.Backup1` over `busctl` (`forge/specs/athanor-backup/athanor-backup-1.0.0/systemd/athanor-backup-hourly.service`), and the backup daemon guards its methods with `org.athanor.backup.*` actions no `.policy` declares, so polkit denies every call (`doc_update_trust.md:125`). A backup that silently never runs is the failure SW6 exists to show.
- The session target wants `xdg-desktop-autostart.target` (`forge/specs/athanor-system-services/SOURCES/usr/lib/systemd/user/athanor-session.target:10`), so XDG autostart entries run as `app-*@autostart.service` units of the user manager.
- `systemd` 258 ships `debug-shell.service`, which starts `/bin/sh` as root on `tty9` without a password (desktop). It matters to SW12.

**Portals.** `xdg-desktop-portal` 1.20.4 is installed. The installed backends are `gtk`, `gnome-keyring` (both `UseIn=gnome`) and ours, which implements ScreenCast, FileChooser, Camera, Location and Microphone (`forge/specs/athanor-xdg-desktop-portal-athanor/xdg-desktop-portal-athanor-1.0.0/athanor.portal:3`). **No installed backend implements `org.freedesktop.impl.portal.Background`** (desktop). The session announces `XDG_CURRENT_DESKTOP=Athanor:COSMIC` (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-session:18`).

**Polkit.** `athanor_bus_api::polkit::check_polkit_auth_zbus` builds the subject from the bus sender (`system/athanor-bus-api/src/polkit.rs:97-104`) and passes an empty details map to `CheckAuthorization` (`polkit.rs:119`), so a polkit rule cannot see which object a request is about. systemd's own actions are `auth_admin_keep` for an active session (`org.freedesktop.systemd1.manage-units`, `manage-unit-files`; desktop). `org.freedesktop.login1.set-self-linger` is `yes` for everyone (desktop). The system journal is readable by `wheel` and `adm` through the ACLs of systemd's `tmpfiles.d/systemd.conf:30-32` (desktop).

**The Gatekeeper** is out of the image until a redesign (`experimental/EXEMPT:36-41`); nothing intercepts execution of code the user obtains.

### 1.3 What the maintainer asked for

Two levels in one application. For the average user: Flatpak at the centre (search, install, update, remove, what is installed and where it comes from, how to remove it); background activity with an on and off switch each, without the word systemd; system services exposed as features, never as units, because the image is immutable and ships its services; failures in plain language with one action. For developers, behind one switch: user services and Podman Quadlet containers in one list, container creation that writes a readable Quadlet file, the user's `nix profile`, and read-only system services whose restart goes through polkit with authentication. Out: Cockpit, a GUI that edits system units in `/etc`, and our own Podman desktop application beyond this. System image updates stay in the shield.

## 2. Decisions

**SW1. One program, one crate.** `athanor-software` in `forge/specs/athanor-software`, a GTK4 program on plain `gtk4-rs` (SH4).

- It is an ordinary application window, not a layer surface, single-instance through `GApplication`. It is not always-on, so it keeps GTK's default renderer; SH4's Cairo rule is for the bar and the dock.
- It links `athanor-style` for tokens and CSS and `athanor-trust-state` to parse the state SW16 reads from `os.athanor.Update1`, and no crate that knows COSMIC (SH2).
- The logic of every page lives in Rust modules with no GTK type, tested without a display, as `doc_bar.md` BR1 does.
- **Long operations are jobs, and a job is a transient unit of the user manager** (`StartTransientUnit`, the mechanism of BR2.3): a Flatpak installation, a `nix profile` operation, an image pull, the Nix catalog build. A job survives the window closing, carries its own resource limits, logs to the journal under its own unit, and is cancelled by stopping the unit. The window only watches it.
- **What a job runs:** the same binary in a job mode, `athanor-software --job <kind> <argument>`, with a closed list of kinds (install, remove, update, delete-data, nix-install, nix-upgrade, nix-remove, pull, nix-index). A job is not a child of the window, so it does not inherit the window's Landlock ruleset; each kind restricts itself at start to the paths it needs (the user Flatpak installation, `~/.var/app/<id>` for delete-data, the Nix profile). The window never writes where a job writes.
- Because the window starts no child process of its own, it restricts itself with Landlock at start, as the greeter and the bar do: read access to its data, the XDG data directories and the journal; write access to `$XDG_CONFIG_HOME/athanor/`, `$XDG_STATE_HOME/athanor-software/`, `$XDG_CONFIG_HOME/containers/systemd/` (SW8) and `$XDG_CONFIG_HOME/autostart/` (SW4).

**SW2. Developer mode is a disclosure switch, not a security boundary.**

- One per-user boolean, `developer_mode` in `~/.config/athanor/software.toml` (`schema = 1`). Off by default.
- Everything it reveals, the user can already do from a terminal. It gates nothing in polkit and no process trusts it.
- Until our Settings exists (stage 6, `doc_shell.md:65`), the switch sits in Software's own preferences. When Settings arrives it moves there and reads the same key; Software's copy is removed.

**SW3. Applications: Flatpak is the centre.** Four pages: Explore, Installed, Updates, and an application's details.

- **Backend:** libflatpak through Rust bindings, and the AppStream data of each configured remote. The window uses it only to read (catalog, installed applications, pending updates); every change runs in a job (SW1). Operations on the system installation go through `flatpak-system-helper` and its own polkit actions; Software adds no action for Flatpak. The bindings are a new workspace dependency, which the plan submits to the maintainer and to `deny.toml` (spike S3).
- **Installation scope.** An application goes to the system installation when a system remote carries it, so updates are shared by every user of the machine. A user who cannot authenticate as an administrator installs into their user installation instead, with no password dialog they cannot answer. Software decides this before offering the button, with a non-interactive `CheckAuthorization` on `org.freedesktop.Flatpak.app-install`: authorised or a challenge means the system installation, a refusal means the user installation. The details page always says which.
- **"Where it comes from":** the remote by its display name (Flathub, Fedora), the installation (for everyone, or only for this user), the version, the installed size, the runtime it uses, and, when the remote states it, whether the developer is verified.
- **What it can reach,** in words, from the permissions in the application's metadata: the network, devices, all files or the home folder, other applications on the session bus. An application with `--filesystem=host` or `home`, or permission to talk to `org.freedesktop.Flatpak`, is described as "Can read and change all your files" and is not called sandboxed, because it can leave its sandbox (`doc_kernel_profile.md:720-722`).
- **Remove:** one button. "Also delete this app's data" (`~/.var/app/<id>`) is an explicit choice, off by default. Runtimes no installed application uses any more are removed with it. The details page also shows the equivalent command, so that a user with a terminal is never locked into the application.
- **Updates:** SW15 and decision 6. The Updates page lists pending application updates, the date of the last successful update run and its failure in plain language (SW6); "Update all" runs one job.
- **Open:** starts the application as `doc_bar.md` BR2 does, as a transient unit. Software cannot create a security context itself when it runs on one (`doc_bar.md:49`), so the application inherits Software's restricted socket: a declared limit (section 5).

**SW4. Background activity.** One page, "Running in the background", two groups. The words systemd, unit, service and daemon never appear in it (SW14).

- **Apps (per user):**
  - Flatpak applications the Background portal reports as running without a window, and those holding the background permission. The switch revokes the permission in the permission store and ends the running instance.
  - XDG autostart entries, the user's and the system's. The switch writes a user entry of the same name with `Hidden=true`, as the XDG autostart specification defines; switching back on removes that entry.
  - Components of the desktop itself (the bar, `athanor-shelld`, the update notifier) are not listed: they are not background applications, and the user cannot run the desktop without them.
- **System features (per machine):** the features of SW5 that run by themselves or listen, with the same switch as SW5.
- **Dependency:** the portal needs a backend that implements `org.freedesktop.impl.portal.Background`, and none is installed (section 1.2). Until one ships, the Apps group lists autostart entries and the applications whose permission is recorded, and says that it cannot see what runs right now. Spike S1 establishes what `xdg-desktop-portal` 1.20 exposes without a backend.

**SW5. Features, not units.** A feature is a named, closed set of units that the image ships.

- Each feature is a data file under `/usr/share/athanor/features/<id>.toml`, schema 1: a permanent English id, a translated name and one sentence, its units, whether it opens a network port to other devices (`exposed`), and whether it is shown only in developer mode. The candidates are decision 2.
- **State** is derived, never stored: on when its units are enabled, and SW6's health on top.
- **Switching** enables and starts the units, or stops and disables them, through the helper of SW12. It writes the enablement links in `/etc`, as a preset does, so the choice survives image updates.
- **"Restore default"** re-applies the image's presets to the feature's units (systemd's `PresetUnitFiles`).
- **No facades (SH1):** a feature whose units are not installed is not shown. File sharing and remote desktop are not features until the image ships a server for them.
- Features are per machine. A user who cannot authenticate as an administrator sees their state and the sentence "An administrator can change this".
- Arbitrary system units are never enabled, disabled or edited. There is no editor of unit files and no view of `/etc`.

**SW6. Failures in plain language, with one action.**

- The health of a feature, an application update run or, in developer mode, a unit is one of a closed list: working; off; not run since a date, when a timer's last trigger is older than twice its period; failed the last time; keeps stopping, when systemd reports `start-limit-hit` or the restart counter grows past a threshold the plan states; waiting for the network.
- Each state has one sentence from our own translations: "Backup has not run since yesterday", "Printing keeps stopping". No exit code, signal name or raw log line is shown at this level.
- One primary action per state: **Retry** clears the failed state and starts the unit once (for a timer, its service once). On a system feature Retry needs the action of SW12. **Show details** opens a sheet with the last and next run, and the last lines of that unit's journal, as plain text.
- The system journal is readable only by `wheel` and `adm` (section 1.2). For other users the details sheet says that the log needs an administrator, and shows the sentence and the times, which come from systemd and need no privilege.
- Software never restarts anything by itself.

**SW7. Developer mode: services and containers in one list.**

- **What is listed:** the units of the user manager, those under `~/.config/systemd/user` and those the Quadlet user generator made from `~/.config/containers/systemd/` (`.container`, `.pod`, `.kube`, `.volume`, `.network`, recognised by the unit's `SourcePath`). A container is a unit, so one list holds both, with a type badge. The desktop's own units are grouped apart and collapsed.
- **Actions:** start, stop, restart; autostart (for a Quadlet unit, the `[Install]` section of its Quadlet file, because generated units cannot be enabled with `systemctl enable`); "Keep running after I log out", which is the user's linger (`org.freedesktop.login1.set-self-linger`, `yes`).
- **Live logs:** the unit's journal, followed, filtered by `_SYSTEMD_USER_UNIT`, in a bounded buffer whose size the plan states, with pause, search and copy. Each line is plain text with ANSI escapes, control and bidirectional characters removed (SH12).
- Everything runs as the user through the user manager on the session bus. No polkit action is involved.

**SW8. Creating a container writes a readable Quadlet file.**

- **The form:** an image reference; a name; ports, each bound to `127.0.0.1` unless the user chooses "Reachable from other devices", which binds all addresses and says that the firewall decides; folders, picked through the file chooser portal, each read-only unless the user unticks it; optional environment variables; restart on failure; autostart.
- **Refusals in the form:** the whole home folder, `/`, `/etc`, `/usr` and `/var` as a folder; host networking; privileged mode; disabling the SELinux label. The form cannot express them.
- **The file:** `~/.config/containers/systemd/<name>.container`, only the keys that were set, `NoNewPrivileges=true` always, and a header comment that says Software wrote it and that it may be edited by hand. It is written to a temporary name, checked with `/usr/libexec/podman/quadlet -dryrun -user`, and renamed only if the check passes; then the user manager is reloaded. The image is pulled as a job (SW1) with progress and cancel.
- **Hand edits win.** Software records the SHA-256 of what it last wrote under `$XDG_STATE_HOME/athanor-software/`. When the file on disk still matches, the form edits it. When it does not, the form is read-only and offers "Edit the file", which opens it in the default text editor; Software never rewrites a file it did not write last.
- **Less isolated:** a Quadlet file, written by hand or not, that sets host networking, a privileged mode, `SecurityLabelDisable`, added capabilities or a mount of a refused path carries a "Less isolated" badge that says which key.
- **Image trust:** no signature is checked for images outside the three system repositories (`doc_update_trust.md:52`). The details say "Not verified", plainly.

**SW9. Developer mode: Nix tools.**

- The page lists the user's `nix profile` (`nix profile list --json`): each package with its flake reference and the locked revision it came from. Install by name, upgrade and remove are jobs (SW1) that run `nix profile` as the user through `nix-daemon`. No polkit action is involved.
- Search uses a local catalog (SW13). Installing needs the network; the page says so when offline.
- Every Nix tool carries the badge of SW10.
- Whether a tool installed this way appears on the session's `PATH` and in the launcher (`~/.nix-profile/share/applications` on `XDG_DATA_DIRS`) is checked in spike S1; Software states what the session does, it does not fix it.

**SW10. Confinement, stated.** Each workload belongs to a tier of `doc_threat_model.md`, and the interface says which (amended 2026-10-06, maintainer decision A2-9 (#151)).

| Workload                  | Confinement today                                                                | What escapes it                                                                              |
| ------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Flatpak application       | bubblewrap namespaces, portals, seccomp                                          | whatever its permissions grant; home or host access leaves the sandbox (SW3)                 |
| Quadlet container         | rootless user namespace, SELinux `container_t`, seccomp, its own cgroup          | mounts the user adds, host networking and the keys SW8 flags; the image itself is unverified |
| Nix tool                  | **none**: it runs as the user, unconfined, like any binary in the home under D23 | everything the user can do                                                                   |
| `toolbox` container       | **none**: privileged, host namespaces, SELinux off (TM5)                         | everything the user can do                                                                   |
| user unit written by hand | **none** by default                                                              | everything the user can do                                                                   |

This document does not claim that Nix tools, hand-written user units or `toolbox` containers are compartmentalised: in the threat model they are tier 1, the user (`doc_threat_model.md`, TM1 and TM5). Their confinement is decision 3 (a): they carry a "Not isolated: runs with all your rights" badge and exist only in developer mode.

**SW11. Who does the work.**

| Operation                          | Performed by                                                  | Runs as           | Authorisation                                                                                                         |
| ---------------------------------- | ------------------------------------------------------------- | ----------------- | --------------------------------------------------------------------------------------------------------------------- |
| Search, details, AppStream refresh | libflatpak; system helper for the refresh                     | user              | `org.freedesktop.Flatpak.appstream-update` (`yes` when active)                                                        |
| Install, remove (system)           | `flatpak-system-helper`                                       | root              | `org.freedesktop.Flatpak.app-install`, `app-uninstall` (`auth_admin_keep`; `wheel` without password by Fedora's rule) |
| Install, remove, update (user)     | a job (SW1) using libflatpak                                  | user              | none                                                                                                                  |
| Background permission, autostart   | permission store over the session bus; files in the home      | user              | none                                                                                                                  |
| Feature on, off, restore, Retry    | `athanor-features` (SW12)                                     | root              | SW12's two actions                                                                                                    |
| System service status              | `org.freedesktop.systemd1` properties                         | user              | none                                                                                                                  |
| System service logs                | the journal                                                   | user              | membership in `wheel` or `adm`                                                                                        |
| System service restart             | `athanor-features`, calling systemd's `TryRestartUnit`        | root              | `os.athanor.services.restart` (decision 1)                                                                       |
| User units, Quadlet, linger        | user manager; files in the home                               | user              | none; `set-self-linger` is `yes`                                                                                      |
| Nix profile                        | `nix` through `nix-daemon.socket`                             | user; daemon root | none                                                                                                                  |
| Running version                    | `os.athanor.Update1.State()`, parsed by `athanor-trust-state` | root service      | none; the reply is trusted only from a name owner whose uid is 0 (SW16)                                               |

No always-on daemon is added. The one new root component is SW12's, activated on demand.

**SW12. The features helper.** Enabling a feature needs root, and systemd's own route is too broad.

- **Why not systemd directly:** `org.freedesktop.systemd1.manage-unit-files` authorises enabling any unit file on the machine. On this image that includes `debug-shell.service`, a root shell on `tty9` with no password (section 1.2). Software would hold that authorisation for the whole keep window of `auth_admin_keep`, and polkit rules cannot narrow it by unit because, as far as can be read, systemd passes no unit to polkit for unit-file changes (spike S1 verifies it).
- **What it is:** `athanor-features`, a root D-Bus service on the system bus, `os.athanor.Features1`, activated on demand and exiting after a short idle period. It ships in the same package as Software.
- **Methods:** `SetEnabled(s feature, b enabled)`, `RestoreDefault(s feature)` and `Retry(s feature)`. The feature id is checked against the files under `/usr/share/athanor/features/`, and only those units are touched. It never accepts a unit name.
- **Authorisation:** the subject is the bus sender through `check_polkit_auth_zbus` with user interaction allowed (`polkit.rs:97-104`). A feature marked `exposed` takes the stricter action (decision 1). No change to `polkit.rs` is needed.
- **Bus policy and hardening** follow `doc_update_trust.md` UT6 and UT1: `send_destination` only with the interface and each `send_member`, no properties, an activation file with `SystemdService=` and `User=root`, the unit hardened with the directives UT1 measured, and `PrivateNetwork=yes`. `verify.py polkit` covers its actions, which are static strings in the `os.athanor.*` namespace.
- The caller's uid, the feature and the result are logged at notice.

**SW13. Size, network and failure.**

- **Flatpak catalog:** search runs on the local AppStream data, never one network query per keystroke. The result list is a recycled `GtkListView` model, so the size of a remote's catalog costs memory in the model, not in widgets. Screenshots are fetched over HTTPS only from the URLs the remote's metadata names, and only when a details page opens.
- **Nix catalog:** nixpkgs holds on the order of 100,000 packages (the maintainer's figure, not measured here), and `nix search` evaluates the whole of it. Software therefore searches a local index built by one job, only when the Nix page is opened and the index is older than the locked nixpkgs revision, never at login. The job runs with `CPUWeight=idle` and a `MemoryMax` the plan measures. Its source (a local evaluation, or the `packages.json` a nixpkgs channel release publishes) is settled by spike S2; a third-party search service is not queried by default, because each query would leave the machine. While no index exists, search says so and installing by exact attribute name still works.
- **Offline:** every page opens from local data. An action that needs the network is disabled with one sentence, "You are offline. Installing needs a connection.", from NetworkManager's connectivity. Nothing is queued to run later without the user seeing it.
- **Slow registry or remote:** every download is a job with progress, the elapsed time and Cancel. A failure is shown as one of a closed list of codes, as in `doc_update_trust.md` UT7 (network, registry, storage, policy, internal), with at most the host name, and Retry.
- **Crash loops:** Software shows what systemd reports (SW6, "keeps stopping", with the count) and offers Stop. It never starts a unit in a loop.
- **Log floods:** the live view keeps a bounded buffer and, when lines arrive faster than it draws, shows how many it skipped.
- **Metered connections:** automatic application updates follow `doc_update_trust.md` UT12: they do not download when NetworkManager's `Metered` is 1 or 3.

**SW14. Untrusted input, accessibility and languages.**

- **Untrusted input:** AppStream text, Flatpak metadata, journal lines, Quadlet files, image references and the JSON of `nix` are set as plain text, truncated and stripped of control and bidirectional characters (SH12). AppStream descriptions keep only paragraphs and lists, rebuilt as widgets, never passed as Pango markup.
- **Accessibility:** every control has an AT-SPI role and name; every flow, including the container form and the log view, works from the keyboard; states are shown by shape and word as well as colour, as the shield's badges are (SH12); high contrast and reduced motion follow SH5. The log view does not announce every line; a change of health is announced once.
- **Languages:** gettext from the first commit; Italian and English shipped; layout mirrored under right-to-left text.
- **Words:** the average-user strings carry a `msgctxt` of their own, and CI fails when any of them contains systemd, unit, service or daemon, in English or in Italian.

**SW15. Application updates run without the window.** The class A updates of `doc_kernel_profile.md:479` need something that runs when Software is closed. The system helper allows an update without a password only from an active session (section 1.2), so a user timer cannot do it. The mechanism is decision 6. Whatever it is, it updates the system installation only: a user installation updates while its owner has a session, from a user timer that runs the update job, and the Updates page says so for each application installed only for this user.

**SW16. System image updates are not here.** Software shows one read-only row: the running version and the sentence of the shield's header, and "System updates are in the shield, at the end of the bar". It reads the state as the bar does since package 2b.5: through the read-only method `State()` of `os.athanor.Update1`, trusted only when the owner of the name that answered has uid 0 (`GetConnectionUnixUser`), and parsed with `athanor-trust-state`. It never reads `/run/athanor-update/state.json` itself: a sandboxed user unit runs in a user namespace where root is not mapped, so the file's owner reads as the overflow uid and the ownership check cannot be made there. It has no button that applies or goes back. Those remain `Apply()` and `GoBack()` of `doc_update_trust.md` UT6, reached from the shield's sheet (`doc_bar.md`, BR6) and the notifier (UT11).

**SW17. Tests.**

- **Surface cases:** the matrix of SH13, scale {1.0, 1.5} × theme {light, dark} × text {English, German, a right-to-left pseudo-locale}, 12 cases per scene. Scenes: Explore with results, an application's details, Installed, Updates with one failure, Running in the background, Features, the services and containers list, the container form, a unit's live log, Nix tools. Ten scenes, 120 cases, estimated at about 16 minutes in series at spike P3's 8 seconds per scene.
- **Fixtures:** the rig has no system services, so a scene starts Software on recorded data fed to the display-free model layer of SW1. That substitution exists only in the test build and never in the shipped binary.
- **Without a display:** unit tests for the feature files, SW6's state machine, the Quadlet writer and its refusals, the hand-edit check, the "Less isolated" detector, the text sanitiser and the forbidden-word check.
- **Functional acceptance** runs on the dev VM (section 8).

## 3. Placement and packages

**Decided (question 4 of section 6):** a standalone application, planned after the switch of stage 2 (`doc_shell.md:211`) as an application track beside stages 3 to 8 (`doc_shell.md:40`). It replaces no shell surface, so SH1's order of surfaces does not place it. It depends on two later stages only for comfort: the polkit agent is cosmic-osd until stage 4 (`doc_shell.md:64`), and the developer-mode switch moves into Settings at stage 6 (SW2).

| Package                                     | Delivers                                                                                                           | Gated by                       |
| ------------------------------------------- | ------------------------------------------------------------------------------------------------------------------ | ------------------------------ |
| **S. Spikes**                               | S1, S2, S3 of section 4                                                                                            | this document approved         |
| **SWa. Applications**                       | the crate, Explore, Installed, Updates, details, removal, SW13 for Flatpak, SW15, SW16; `athanor-store-rs` deleted | S1, S3, decisions 5 and 6 |
| **SWb. Background and features**            | SW4, SW5, SW6, `athanor-features` with its `.policy` and bus files, the feature files, remote login off on new installs                              | SWa, decisions 1 and 2    |
| **SWc. Developer: services and containers** | SW2, SW7, SW8, system service status, logs and restart                                                             | SWb                            |
| **SWd. Developer: Nix tools**               | SW9, the Nix catalog                                                                                               | SWc, S2, decision 3       |
| **SWe. Default applications**              | the RPM defaults of decision 7 in `athanor-desktop-ui`, the removals, the favourites, the MIME and terminal lists, the Terminal action; Papers once its conditions hold | decision 7; Papers also SWa and `doc_portal.md` |

- Each package ships alone, enabled by hand, and is useful alone.
- **The switch, at the end of SWa:** when Software passes SH1's rule against `cosmic-store` on the dev VM and on the maintainer's desktop, `cosmic-store` leaves the image (decision 5).

## 4. Spikes

Each runs before the plan it gates and produces an answer, not code we keep.

| #   | Spike                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | Settles                              |
| --- | -------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- | ------------------------------------ |
| S1  | On the dev VM, on the current `iso-v0` image: (1) whether systemd 258 passes the unit to polkit for `EnableUnitFiles` and for `RestartUnit`; (2) what `xdg-desktop-portal` 1.20 exposes for background applications with no Background backend, and which backend could supply one without GNOME; (3) whether Flatpak reads a remote from a file under `/etc/flatpak/remotes.d/` and `/usr/share/flatpak/remotes.d/`; (4) whether a non-`wheel` user reads their own user journal and what they see of the system journal; (5) whether a `nix profile` tool reaches the session's `PATH` and the launcher; (6) whether `quadlet -dryrun -user` runs under Software's Landlock ruleset, or must itself be a job | SW4, SW6, SW9, SW12, decision 6 |
| S2  | The Nix catalog: time, peak memory and disk of a local evaluation of nixpkgs on a v3 laptop with 8 GB, against downloading a channel's `packages.json`; how either maps to the revision `nix profile` locks                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                    | SW13, the job's `MemoryMax`          |
| S3  | The libflatpak Rust bindings and an AppStream reader: maintenance, licence against `deny.toml`, and whether they build against Fedora 43's libraries in the workspace                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                                          | SW3                                  |

## 5. Risks and declared limits

- **Nix tools and hand-written user units run unconfined** (SW10). The badge informs; it does not protect.
- **Developer mode is not a boundary** (SW2). A process running as the user can flip it.
- **"Open" from Software** starts the application on Software's own restricted socket when Software was started from the bar, so the context names Software's app id, not the application's. Stage 3's launcher, and a launch request to the bar if one is ever designed, remove the limit.
- **User images are unverified** (SW8). A signed-image policy for user containers is a later design entry.
- **The Background portal has no backend today** (SW4). Until one ships, Software cannot list what runs in the background right now, and says so.
- **Two snapshot timers are enabled,** `athanor-timewarp.timer` and `athanor-backup-hourly.timer` (`80-athanor-system.preset:9-10`), and the second fails by construction (section 1.2). Open question 2 must name one "Backup" feature, not two.
- **Scope.** Four packages for one maintainer. The brake is the same as the shell's: each package ships alone and enabled by hand, and `cosmic-store` stays until SWa passes SH1's rule.
- **Archives are parsed unconfined while Nautilus is the interim file manager.** It extracts through libarchive in its own process, with all of the user's rights (decision 7); the risk would end if Athanor's file manager replaced Nautilus after 1.0, which is intended, pending the maintainer (`doc_files.md` section 7).
- **A new file manager can lose data.** A move across filesystems, a copy interrupted halfway and a name conflict are where file managers lose files; `doc_files.md` makes each an acceptance case before the file manager replaces Nautilus.
- **GNOME Disks is GTK3** while it is the interim disk utility: it ignores the accent and looks unlike the other defaults (decision 7).
- **A disk utility destroys data by design.** Formatting the wrong disk cannot be undone; `doc_disks.md` makes the choice of the target disk and its confirmation acceptance cases, tested on virtual disks, before the utility replaces GNOME Disks.
- **The default applications ignore Athanor's appearance** until a Settings portal backend serves it (decision 7).
- **libflatpak** is a C library with a large surface in Software's process. Its input is AppStream data from remotes, which SW14 treats as untrusted.

## 6. Decisions of the maintainer (2026-09-30)

1. **The new polkit actions.**
   - Proposed, both used only by `athanor-features` (SW12), in one `os.athanor.features.policy`:
     - `os.athanor.features.change`: switch, restore or retry a feature that opens no port. Defaults `auth_admin_keep` / `auth_admin_keep` / `auth_admin_keep`.
     - `os.athanor.features.change-exposed`: the same for a feature marked `exposed` (remote login, network discovery). Defaults `auth_admin` for all three, never kept.
   - For restarting a system service in developer mode: (a) systemd's own `org.freedesktop.systemd1.manage-units`, `auth_admin_keep`, no new code; (b) a third method on the helper, `RestartService(s unit)`, under a new `os.athanor.services.restart`, `auth_admin`, never kept, which calls systemd's `TryRestartUnit`: it restarts a unit that is running and never starts one that is not.
   - One action per feature was considered and set aside: the names would be built at run time, which `verify.py polkit` cannot check, and a single action with the feature as a detail would need `polkit.rs` to pass details (`polkit.rs:119`), a protected file.
   - **Decided:** the two feature actions, and (b) for restart. Option (a) has the breadth SW12 rejects for unit files: `manage-units` authorises `StartUnit` on any unit, `debug-shell.service` included, and Software would hold it for the whole keep window. A path under (b) never starts a stopped unit, so it cannot bring up the debug shell. Refusing units outside `/usr/lib/systemd/system` would not be enough, because `debug-shell.service` lives there too.
   - Separately from Software: mask `debug-shell.service` in the image, so that no route, systemd's own included, can start it on a production image. A debug build can unmask it.
   - `os.athanor.store.install` leaves with `athanor-store-rs`.
2. **The features for average users,** drawn from what the image ships (section 1.2).

   | Id                 | Name                        | Units                                                          | Default today | Exposed | Source                                             |
   | ------------------ | --------------------------- | -------------------------------------------------------------- | ------------- | ------- | -------------------------------------------------- |
   | `printing`         | Printing                    | `cups.socket`, `cups.path`                                     | on            | no      | Fedora `90-default.preset:59-60`                   |
   | `remote-login`     | Remote login (SSH)          | `sshd.service`                                                 | on, key only  | yes     | `90-default.preset:25`; `athanor-install.ks:37-38` |
   | `discovery`        | Find devices on the network | `avahi-daemon.socket`, `avahi-daemon.service`                  | on            | yes     | `90-default.preset:54`                             |
   | `backup`           | Backup of your files        | one of `athanor-backup-hourly.timer`, `athanor-timewarp.timer` | on            | no      | `80-athanor-system.preset:9-10`                    |
   | `mobile-broadband` | Mobile broadband            | `ModemManager.service`                                         | on            | no      | `90-default.preset:39`                             |
   | `smart-cards`      | Smart card readers          | `pcscd.socket`                                                 | on            | no      | `90-default.preset:227`                            |
   | `nix` (developer)  | Nix package manager         | `nix-daemon.socket`                                            | on            | no      | `80-athanor-nix.preset:14`                         |
   - Not features: firewalld, usbguard, boltd, Tetragon and the update units, which are security and are not switched off from an application; Bluetooth, which the bar owns (`doc_bar.md`, BR3); file sharing and remote desktop, which the image does not ship (SH1: no facades).
   - **Decided:** the table as it stands, with `backup` bound to whichever snapshot mechanism the backup rewrite keeps, and `smart-cards` shown only when a reader is present. Remote login is **off** by default on new installs: the kickstart stops enabling `sshd` (`system/athanor-install.ks:37-38`) and stops opening the SSH port in the same package that ships the feature switch (SWb), so that turning it on never needs a terminal. Existing installs keep their state. Revised on 2026-10-07: the default moved ahead of SWb, set by the ISO's kickstart (`system/disk_config/iso.toml`, `services --disabled=sshd`) and not by the image's preset, so that installed machines keep sshd through the ostree `/etc` merge; until SWb ships, turning it on is `systemctl enable --now sshd.service`.

3. **Confinement of Nix and Quadlet workloads.**
   - (a) **Declared exception:** Nix tools and hand-written units stay unconfined, only in developer mode, with the badge of SW10, and the exception is written into the project rule by the maintainer (section 7). Amended 2026-10-06 (maintainer decision A2-9 (#151)): no exception is needed, because the threat model makes these workloads tier 1, the user (`doc_threat_model.md`, TM1).
   - (b) **Per-tool sandbox:** run every Nix tool through a bubblewrap or Landlock wrapper. A generic profile either breaks developer tools, which need the home and the network, or restricts nothing.
   - (c) **A container for untrusted tools:** Nix inside a rootless podman container with SELinux `container_t` and SW8's defaults, or the dev VM, with only the profile's result exported. It isolates, at the cost of friction. Amended 2026-10-07 (ADR-0086, which amends A2-9, #151): a `toolbox` container, which this option first named, is not isolation and leaves this decision; rootless podman confined as `container_t`, or the dev VM, is the isolation. toolbox creates it privileged, in the host's process and network namespaces, with SELinux separation disabled and the host's root mounted at `/run/host` (`doc_threat_model.md`, TM5); code in it is tier 1, and where Software shows one it carries SW10's "Not isolated" badge.
   - (d) **For Quadlet:** rootless podman with SW8's defaults and refusals, and the "Less isolated" badge on anything weaker.
   - **Decided:** (a) now, with (c) documented as the way to run a tool one does not trust, and (d) for containers. A real confinement of user workloads belongs with the confinement of launched applications that `doc_bar.md` BR2 already defers.
4. **Placement.**
   - (a) A standalone application (section 3).
   - (b) Pages of our future Settings (stage 6).
   - (c) Applications standalone, features and developer tools in Settings.
   - **Decided:** (a). "What runs on my machine and where it came from" is one question and belongs in one place; Settings is three stages away and should not hold Flatpak back. Only the developer-mode switch moves into Settings.
5. **The fate of `athanor-store-rs`, `system/athanor-store` and `cosmic-store`.**
   - `athanor-store-rs`: extend, rename, or delete. **Decided: delete it** in SWa, per SH4's rule for legacy crates (`doc_shell.md:76`). Nothing is worth mining: its function is two `flatpak` invocations, and its signature check is a facade (section 1.2).
   - `system/athanor-store`: **Decided:** remove its `install` and `disconnect-flathub` commands, which hold a placeholder digest in a security path. Its storage engine belongs to the mesh and is decided with the mesh, not here.
   - `cosmic-store`: keep it for good as COSMIC content (SH3), or remove it at SWa's switch. **Decided: remove it at the switch.** Two stores would give two answers to "where does it come from" and two update paths for the same installation.
6. **Flatpak remotes and automatic application updates.**
   - Remotes: (a) Fedora's only, as today; (b) Flathub added as a system remote by a file the image ships, beside Fedora's; (c) as (b), limited to Flathub's verified subset. The `flatpaks` list of `packages.json` either gets an installer run on first boot or is deleted.
   - Updates: (i) a hardened system oneshot timer running the system update, with the metered rule of UT12; (ii) updates only while a session is active, from a user service; (iii) a polkit rule that lets a user timer update without a session.
   - **Decided:** (b) and (i). Without Flathub the Explore page would be close to empty, and a system timer is the only one of the three that updates a machine nobody is logged in to, without widening polkit. The `flatpaks` list is deleted: a machine that installs Chrome and Spotify unasked is not "for everyone". Amended by decision 7: the list is replaced by the image's list of preinstalled defaults, which holds no proprietary application.

7. **Default applications (2026-10-05).**
   - On 2026-09-25 the maintainer set the end state of the desktop: cosmic-comp is the only COSMIC component left (`doc_shell.md` revision 5, SH1 and SH3). On 2026-10-05 the maintainer extended it to COSMIC's applications, which SH3 still calls content (`doc_shell.md:73`): each is replaced by the best available application for its use. Two exceptions, decided the same day: the file manager is Athanor's own (`doc_files.md`, to be written), and so is the disk utility (`doc_disks.md`, to be written). Every file manager may write all of the user's files, but Nautilus also parses archives in its own process, reaches the network and the whole session bus, and may write the places where an attack persists (autostart, shell profiles, user units). Athanor's is built with privilege separation from the start, like the bar and the greeter: a confined interface, untrusted parsers in sandboxed helpers. Its cost is measured, not guessed: the shell written so far is about 25,000 lines of Rust in ten days (2026-09-23 to 2026-10-04); an essential file manager is of the bar's order (13,248 lines), parity with Nautilus (MTP, SMB and SFTP, conflict dialogs, undo, batch rename, every translation) takes months and is not the goal. The disk utility is chosen for the third criterion, not for confinement: GNOME Disks, like any client, asks the privileged `udisks2` service, under polkit, for every change to a disk, and Athanor's will ask the same service; what it adds is a GTK4 interface now, while GNOME's port waits upstream. What only Athanor can offer around files: the file chooser (`doc_portal.md`, to be written), which shares the file manager's view, and the previous versions of a file from the btrfs backup.
   - Criteria, in order: works with Orca and with the keyboard alone; native Wayland on cosmic-comp with fractional scaling; follows the colour scheme, the accent and the contrast of the XDG Settings portal; untrusted file parsers confined; a release in 2026 and not deprecated upstream; a Fedora 43 RPM or a verified Flathub build; translations; footprint. GTK4 with libadwaita meets the third criterion without a bridge and matches the shell's toolkit (SH4), once the session serves the Settings portal (below).
   - Checked on 2026-10-05: the Fedora 43 RPMs with `dnf repoquery` (nautilus 49.6, ptyxis 49.3, gnome-text-editor 49.2, loupe 49.2, papers-thumbnailer 49.8, showtime 49.1, decibels 49.6.1, gnome-calculator 49.2, gnome-system-monitor 49.1, gnome-disk-utility 46.1, gnome-font-viewer 49.0, snapshot 49.1); the `finish-args` of each Flathub manifest under `github.com/flathub`; the upstream tags on `gitlab.gnome.org`; the portal and the packages on the maintainer's desktop. The accessibility and translation claims come from upstream documentation; acceptance item 16 confirms them.

   | Use                   | Default              | Channel                          | Replaces                                     | Note |
   | --------------------- | -------------------- | -------------------------------- | -------------------------------------------- | ---- |
   | Files                 | Athanor's file manager (`doc_files.md`) | custom RPM            | `cosmic-files`, `Thunar`, `thunar-volman`    | Until it ships, Nautilus, which `athanor-desktop-ui` already requires, is the interim default; it leaves when the file manager ships. Nautilus is not on Flathub (checked 2026-10-05), so Software cannot offer it afterwards. Search goes through `localsearch`, which `athanor-launcher` also requires; its upstream defaults already index only the user's folders, skip hidden files and ignore removable media. |
   | Archives              | Athanor's file manager | custom RPM                     | `thunar-archive-plugin`                      | Extraction runs in a sandboxed helper that may write only the destination folder (`doc_files.md`). With the interim Nautilus it runs in process, through gnome-autoar and libarchive, unconfined (section 5). File Roller is not a default: its Flathub build may write the whole home, so it would confine nothing. |
   | Terminal              | Ptyxis               | RPM                              | `cosmic-term`, `foot`                        | Opens shells in toolbox and podman containers, where development on an immutable image happens. It is the terminal `xdg-terminal-exec` chooses, which the launcher already runs (`system/athanor-compositor-client/src/launch.rs:22`) and the compositor's Terminal action runs too. |
   | Text editor           | GNOME Text Editor    | RPM                              | `cosmic-edit`                                |      |
   | Images                | Loupe                | RPM                              | `imv`                                        | Decodes through glycin, which runs each image loader under bubblewrap with a seccomp filter. That needs the `bwrap` binary and unprivileged user namespaces, which the image has (section 1.2); without them glycin runs the loader unconfined and only logs a warning, which acceptance item 16 checks for. |
   | Documents (PDF, EPUB) | Papers               | Flatpak; `papers-thumbnailer` RPM | nothing; no document viewer ships today     | Parses untrusted documents in process. Its Flathub build reads the home read-only and has no network, so a compromised parser can neither change files nor send them away. That build carries no thumbnailer, so the RPM gives the file manager its PDF previews. |
   | Video                 | Showtime             | RPM                              | `mpv`                                        | Plays through GStreamer. The patented codecs come from `gstreamer1-plugin-libav` over RPM Fusion's `ffmpeg-libs`; the plugin is on the desktop but in no list of `packages.json`, so it is required by name. |
   | Music                 | Decibels             | RPM                              | nothing                                      | Plays single files. A library player is the user's choice in Software. |
   | Calculator            | GNOME Calculator     | RPM                              | nothing                                      |      |
   | System monitor        | GNOME System Monitor | RPM                              | nothing                                      | Resources was weighed: Fedora does not package it, and its Flathub build reads the whole host, has the network and may talk to `org.freedesktop.Flatpak`, which SW3 calls not sandboxed. |
   | Disks                 | Athanor's disk utility (`doc_disks.md`) | custom RPM            | nothing                                      | Until it ships, GNOME Disks 46.1 is the interim default and a **declared exception** to the third and fifth criteria: GTK3 with libhandy, it does not take the accent. Its GTK4 port exists only as 51.beta (2026-07-30) and missed GNOME 51.0 (2026-09-14) with no release candidate (upstream issue 505). It is not on Flathub (checked 2026-10-05). It ships in the interim because formatting a USB stick needs a graphical tool. |
   | Fonts                 | GNOME Font Viewer    | RPM                              | nothing                                      |      |
   | Camera                | Snapshot             | RPM                              | nothing                                      |      |
   | Web                   | Firefox              | RPM                              | `org.mozilla.firefox` in the `flatpaks` list | Installed today because `athanor-desktop-ui` requires it; the list never installed the Flatpak (section 1.2). |
   | Applications          | Software             | this document                    | `cosmic-store`, at SWa's switch (decision 5) |      |

   - **Not installed, found in Software:** calendar, contacts, email, office suite, an archive manager, scanning, remote desktop, password managers, development environments, and the proprietary applications the `flatpaks` list named (Google Chrome, Spotify, Visual Studio Code). A machine installs no proprietary software unasked.
   - **Where the list lives.** The RPM defaults are `Requires` of `athanor-desktop-ui`, which already requires Nautilus and Firefox (`athanor-desktop-ui.spec:21`); Nautilus gives way to the file manager when it ships: one package states the desktop, and a change of the base image cannot drop an application unnoticed. `packages.json` loses the replaced packages and gains none of these.
   - **The preinstalled Flatpak.** Papers is the only Flatpak default. The image names it in a `*.preinstall` file under `/usr/share/flatpak/preinstall.d/`, the format Flatpak 1.17.0 reads for its `flatpak preinstall` command. The image carries Flatpak 1.16.6, which lacks the command, so until it carries 1.17 or later the system update service of decision 6 (i) reads the same files and installs what they name, once per machine, system-wide, from Flathub. A default the user removes is recorded and never installed again; one that fails to install is retried by the next run and reported by Software, never skipped silently. When the image's Flatpak has the command, the service calls it instead, provided it also keeps a removal; the plan checks that.
   - **Three conditions before the defaults are complete.**
     - The Flathub remote (decision 6, package SWa), for Papers.
     - A FileChooser backend that works, for Papers. Since PR #107 the Athanor backend, the only FileChooser backend for `XDG_CURRENT_DESKTOP=Athanor` (`athanor.portal` has `UseIn=athanor`, `gtk.portal` has `UseIn=gnome`), cancels every open and refuses every save (`portal.rs`, `OpenFile` and `SaveFile`), so the Open and Save dialogs of every Flatpak application fail.
     - A Settings backend that serves Athanor's appearance, for every default. On the maintainer's desktop on 2026-10-05, with no backend for `Athanor` and no `portals.conf`, `xdg-desktop-portal` 1.20.4 falls back to the gtk backend, which answers `color-scheme` 0 (no preference) and `contrast` 0 and does not know `accent-color`. libadwaita and GTK4 read these keys from the portal in sandboxed and unsandboxed applications alike, so today every default draws light, with libadwaita's own accent, whatever the shell shows.
     - `doc_portal.md` decides both backends and `doc_visual_language.md` the values the Settings backend serves. The RPM defaults ship without waiting; acceptance item 16 passes only when all three hold.
   - **Wired by the image:** the favourites of a new user (`forge/specs/athanor-bar/athanor-bar-1.0.0/data/favorites.toml`) become the file manager (Nautilus until it ships), Ptyxis, GNOME Text Editor and Settings; `/usr/share/applications/athanor-mimeapps.list`, which the XDG lookup reads before Fedora's lists because the session's first desktop name is `Athanor`, opens each type with the application of the table; `/usr/share/xdg-terminal-exec/athanor-xdg-terminals.list` names Ptyxis; the compositor's Terminal action runs `xdg-terminal-exec` instead of COSMIC's `cosmic-term`, set in the image's COSMIC defaults under `/usr/share/athanor/cosmic-defaults`, where the theme already lives. The icon theme is decided in `doc_visual_language.md`.
   - **Removed from the image:** `cosmic-files`, `cosmic-term`, `cosmic-edit`, `Thunar`, `thunar-archive-plugin` and `thunar-volman` from `upstream_desktop`; `mpv` and `imv` from `upstream_media`; `cosmic-store` at SWa's switch. `foot` leaves with `athanor-shell-rs`, not before: `athanor-shell-rs.spec:11` requires it and the legacy shell's search runs it. `athanor-shell-rs` is shipped and started by nothing since the greeter moved to `athanor-greeter-ui`; the portal still requires it (`xdg-desktop-portal-athanor.spec:13`), although its chooser no longer runs it, so it leaves with `doc_portal.md`.

## 7. Changes to other documents

Each change lands with the package named beside it, not with this document.

- `doc_shell.md`, SH3 (`doc_shell.md:73`): `cosmic-store` leaves the image at the switch of package SWa (decision 5). With SWa.
- `doc_shell.md`, section 3, "Later stages": a pointer to this document as an application track after stage 2. With the first plan (package S).
- `experimental/EXEMPT:18-22`: the comment on `athanor-store-rs` is removed with the crate. With SWa.
- `CLAUDE.md`, "Limiti inviolabili": the compartment-or-MicroVM rule is replaced by the service rule of `doc_threat_model.md`, TM8, in text the maintainer reviews; decision 3 (a) needs no exception (amended 2026-10-06, maintainer decision A2-9 (#151)). Before SWc.
- `forge/config/packages.json`: the `flatpaks` list and `system/scripts/provision_flatpak.sh` are deleted (decision 6). With SWa.
- `forge/specs/athanor-desktop-ui/athanor-desktop-ui.spec`: requires the RPM defaults of decision 7 and no longer `foot`; ships `athanor-mimeapps.list`, `athanor-xdg-terminals.list` and the preinstall file for Papers. With SWe.
- `forge/config/packages.json`, `upstream_desktop` and `upstream_media`: the removals of decision 7. With SWe.
- The image's COSMIC defaults (`/usr/share/athanor/cosmic-defaults`, package `athanor-calmo`): the Terminal action. With SWe.
- `forge/specs/athanor-bar/athanor-bar-1.0.0/data/favorites.toml`: the favourites of decision 7. With SWe.
- `doc_shell.md`, SH3 (`doc_shell.md:73`): COSMIC's applications no longer stay as content; they leave as decision 7 says. With SWe.
- `scripts/devvm/launcher-acceptance.sh`: opens Ptyxis instead of `cosmic-term`. With SWe.
- `doc_files.md`, to be written: Athanor's file manager, its sandboxed helpers and its data-loss acceptance cases (decision 7). Its package removes `nautilus` from `athanor-desktop-ui`.
- `doc_disks.md`, to be written: Athanor's disk utility over `udisks2` and its acceptance cases for destructive operations (decision 7). Its package removes `gnome-disk-utility` from `athanor-desktop-ui`.
- `doc_portal.md`, to be written: the FileChooser and Settings backends of decision 7, and the removal of `athanor-shell-rs`, of the portal's `Requires` on it and of `foot`.
- `system/athanor-install.ks`: `sshd` no longer enabled and the SSH port no longer opened on new installs (decision 2). With SWb.
- `system/athanor-store`: the `install` and `disconnect-flathub` commands are removed (decision 5). With SWa.
- The system image: `debug-shell.service` is masked (decision 1). Independent of Software, in its own change, as soon as possible.

## 8. Acceptance

On a fresh install in the dev VM, and on the maintainer's desktop upgraded in place:

1. An application is found by name offline from the local catalog, installed from Explore, started, and removed from Installed, with and without its data. The details page names its remote, its installation, its version and what it can reach; an application with home access is not called sandboxed.
2. A user who is not an administrator installs an application into their user installation with no password dialog.
3. With the network down, every page opens and every action that needs the network is disabled with its sentence.
4. Application updates download with nobody logged in, and not on a connection marked metered.
5. Turning an autostart entry off writes a `Hidden=true` user entry and it does not start at the next login; turning it on removes the entry.
6. Turning "Printing" off stops and disables its units and asks for administrator authentication; turning "Remote login" off asks again even within the keep window; "Restore default" re-applies the image's presets. `busctl` cannot make `athanor-features` touch a unit outside a feature file, nor reach any other member.
7. With the backup unit made to fail, the Background page reads "Backup has not run since" and a date, and Retry runs it once. With a unit made to crash in a loop, it reads "keeps stopping" and Software starts nothing by itself.
8. The English and Italian average-user catalogs contain none of the forbidden words; CI proves the check fails on a planted one.
9. In developer mode, a container created from the form runs, its file is readable and passes `quadlet -dryrun -user`, and its port answers on `127.0.0.1` only. After a hand edit the form is read-only and the file is never rewritten. A hand-written file with host networking carries "Less isolated".
10. The list shows the user's units and the Quadlet units together; start, stop, restart, autostart and "Keep running after I log out" work; the live log follows a unit and survives a flood.
11. A system service's status and log are shown read-only to a member of `wheel`; its restart asks for administrator authentication.
12. A tool is installed into, upgraded in and removed from the user's `nix profile`; the page shows its locked revision and the "Not isolated" badge; search answers from the local index without evaluating nixpkgs again.
13. The running version is shown read-only with the shield's sentence, and Software offers no way to apply or go back.
14. The AT-SPI tree exposes a role and a name for every control; every flow completes from the keyboard; Orca reads a change of health once. Both languages run.
15. The 120 surface cases of SW17 pass in CI; `verify.py polkit`, `verify.py shipped` and the unit tests of the crate pass.
16. On a fresh install, every default application of decision 7 opens a file of its type from the file manager, an archive included, which is extracted by the sandboxed helper of `doc_files.md`; the file manager shows a PDF thumbnail; the journal holds no glycin warning that a loader ran without its sandbox; the compositor's Terminal action and the launcher both open Ptyxis. Every default is read by Orca and is usable from the keyboard alone, at scale 1.0 and 1.5. Once the Settings backend of decision 7 ships, every default but the interim GNOME Disks follows a change of colour scheme, accent and high contrast without a restart. No package removed by decision 7 is installed, and Papers, removed by the user, is still absent after two runs of the update service.

## 9. Amendments of 2026-10-06

Maintainer decisions A2-14, A2-15 and A2-24 (#159), from specification audit 2 of 2026-10-05. They override sections 1 to 8 where they disagree.

### 9.1 Disks: GNOME Disks for good

The Disks row of decision 7 now reads: **GNOME Disks**, Fedora RPM `gnome-disk-utility`, replacing nothing. Athanor does not write its own disk utility (`doc_disks.md` section 7); GParted stays out because its whole interface runs as root through `pkexec` (maintainer decision A2-16 (#155); #159 repeats it).

- GNOME Disks 46.1 (GTK3 with libhandy) remains a **declared exception** to the third and fifth criteria until the GTK4 port is released and reaches Fedora. Upstream on 2026-10-06: tag `51.beta` (2026-07-30), no `51.0`, work item 505 open (`https://gitlab.gnome.org/GNOME/gnome-disk-utility/-/work_items/505`). The GTK4 release with GNOME 52 is audit 2's estimate.
- **Channel.** As decision 7 sets for every RPM default, GNOME Disks ships as a `Requires` of `athanor-desktop-ui`, beside `nautilus`, `ptyxis` and `gnome-text-editor`; `packages.json` gains nothing. Today nothing requires it and the base image does not carry it; the image change lands with PR #169.
- `athanor-mimeapps.list` maps the disk image types to GNOME Disks's own entries, `gnome-disk-image-mounter.desktop` and `gnome-disk-image-writer.desktop` (both in the Fedora 43 package, `mdapi.fedoraproject.org`, 2026-10-06), not to `athanor-disks`.
- Section 5's risk "GNOME Disks is GTK3" holds until the GTK4 release; acceptance item 16 keeps its exception for it until then.

### 9.2 Files: Nautilus through 1.0

The Files and Archives rows of decision 7 read: **Nautilus is the file manager of 1.0**, with the declared risk of `doc_files.md` section 7 (extraction in process through gnome-autoar and libarchive, the network and the whole session bus, writes to the paths where an attack persists, system extensions and user scripts). Until 1.0 Athanor builds only `athanor-files-view`, the view the FileChooser backend of `doc_portal.md` needs. The rest of `doc_files.md` waits until after 1.0 and the scope budget of `doc_shell.md` SH3. Acceptance item 16 runs with Nautilus: the archive is extracted by Nautilus, unconfined, and "by the sandboxed helper of `doc_files.md`" applies only once that file manager ships.

### 9.3 Firefox: the Flatpak from Mozilla

The Web row of decision 7 now reads: **Firefox**, channel **Flatpak**, `org.mozilla.firefox` from Flathub (A2-15, #159), replacing the Firefox RPM.

- **The build.** Read from the Flathub API on 2026-10-06 (`https://flathub.org/api/v2/appstream/org.mozilla.firefox`): developer Mozilla, verified by Flathub, licence MPL-2.0. Its filesystem permissions are the downloads folder, read-only GTK 3 settings, the Kerberos credential socket and the speech-dispatcher socket: no home folder and no host. It has the network, every device, Wayland, X11, PulseAudio, PC/SC and CUPS; on the session bus it may talk to gvfs, the accessibility bus and `org.freedesktop.FileManager1`; on the system bus to NetworkManager. By SW3's rule it is sandboxed, and its details page says it reaches the network and all devices.
- **Why.** An RPM browser runs unconfined as the user; the Flatpak keeps the parser that meets the most untrusted input out of the home folder, as Papers does for documents (security audit 2: the RPM browser is in the class the persistence finding names).
- **What the user gets today.** The base image `quay.io/fedora-ostree-desktops/base-atomic:43` itself ships `firefox` and `firefox-langpacks` as RPMs: on the maintainer's desktop (image 43.20261004.153) both carry the install time of the base layer, not of Athanor's layer, and `rpm -q --whatrequires firefox` names `firefox-langpacks` and `athanor-desktop-ui`. `athanor-desktop-ui.spec:21` also requires it. The `flatpaks` list of `packages.json` already names `org.mozilla.firefox`, but nothing installs that list (section 1.2; `provision_flatpak.sh` is never run, PR #169), so no machine has the Flatpak. Today every user gets the Firefox RPM.
- **The order of the change** (a proposal awaiting the maintainer), so that no machine is ever left without a browser: (1) the system update service of decision 6 (i) reads the preinstall files and installs from Flathub (package SWa), and Firefox gets a `*.preinstall` file beside Papers's (decision 7, "The preinstalled Flatpak"); (2) in the same change, `athanor-desktop-ui` drops `Requires: firefox`, the image build removes the base image's `firefox` and `firefox-langpacks`, and `athanor-mimeapps.list` and the favourites name `org.mozilla.firefox.desktop`. Dropping the `Requires` alone removes nothing, because the base image carries the RPM. Until step (1) the RPM stays.
- **A machine updated from an earlier image** loses the RPM at step (2) and gets the Flatpak from the first run of the update service; the profile under `~/.mozilla` is not migrated by this document. Whether the image offers Mozilla's own profile import or a one-time copy into `~/.var/app/org.mozilla.firefox` is left to the plan of step (2), and it is an acceptance case there.
- Firefox's row joins the preinstalled Flatpaks: "Papers is the only Flatpak default" now reads "Papers and Firefox are the Flatpak defaults". Decision 6's deletion of the `flatpaks` list stands; until the preinstall files replace it, `org.mozilla.firefox` stays named there.

### 9.4 Software and Bazaar: the written comparison

Maintainer decision A2-14 (#159) asks for a written comparison with Bazaar before this document is approved. The working hypothesis of #159 is Bazaar for applications, with an Athanor page for developer mode, the Quadlet view and the confinement badges. This section gives the facts and a recommendation; the choice is the maintainer's.

**Bazaar, read on 2026-10-06:**

- Repository: `https://gitlab.gnome.org/World/bazaar` (moved from `https://github.com/kolunmi/bazaar`, now archived, on 2026-08-18). Licence GPL-3.0, written in C. Latest tags `v0.9.7` (2026-10-03) and `v0.9.6` (2026-09-18): pre-1.0, released about every two weeks.
- Build dependencies (`src/meson.build` at `v0.9.7`): GTK 4.22.1 or later, libadwaita 1.8 or later, libflatpak, libappstream, WebKitGTK 6.0, malcontent, libsecret, glycin 2, libsoup 3, libsystemd.
- Scope (`docs/overview.md` at `v0.9.7`): Flatpak remotes only; Flathub's data and Flathub sign-in; a queue of transactions; a curated front page in YAML that the distribution writes; a blocklist of application IDs; hooks (`view-app`, `article-app`, `before-transaction`, `after-transaction`) that run shell snippets the distribution configures; `hide-auto-update-options`, which hides its own automatic updates "if your distro handles auto-updates of Flatpaks already". A background `bazaar-daemon --no-window` started by an autostart entry keeps the catalogue warm and serves the shell's search through `org.gnome.Shell.SearchProvider2`.
- Permissions shown to the user: `src/bz-app-permissions.c` reads an application's metadata and flags it as able to escape its sandbox for the session or system bus socket, the `gpg-agent` socket, `xdg-data/flatpak/overrides:create`, and talking to `org.freedesktop.Flatpak` or to the permission store: close to SW3's rule, written independently.
- Translations: 40 files under `po/` at `v0.9.7`, Italian and Arabic among them.
- Packaging: Fedora 43, 44 and rawhide do not package it (`mdapi.fedoraproject.org`, 2026-10-06). Flathub carries `io.github.kolunmi.Bazaar` (`https://github.com/flathub/io.github.kolunmi.Bazaar`, last commit 2026-10-03 "v0.9.7"): runtime `org.gnome.Platform` 51; `finish-args` include the network, `/var/lib/flatpak`, `xdg-data/flatpak`, `/var/tmp`, `~/.var/app`, `--talk-name=org.freedesktop.Flatpak`, `--system-talk-name=org.freedesktop.Flatpak.SystemHelper` and `org.freedesktop.Accounts`. By SW3's rule that build is not sandboxed: talking to `org.freedesktop.Flatpak` lets it run any command on the host. Inside its sandbox it reads the distribution's configuration from `/run/host/etc/bazaar/` (`bazaar.yaml`, `config.yaml`, `blocklist.txt`), which needs a Flatpak override granting that path, shipped by the image under `/var/lib/flatpak/overrides` through tmpfiles, as Bluefin and Aurora do.
- Adoption: Bluefin ships it and calls a distribution's own store unsustainable (`https://github.com/ublue-os/bluefin-docs`, `docs/index.md`, lines 58 and 75).

**Against this document's requirements:**

| Requirement | Bazaar | Athanor's Software |
| --- | --- | --- |
| SW3 Explore, Installed, Updates, details, remove | yes; curated front page, queue | to write (package SWa) |
| SW3 permissions in words, "not sandboxed" | yes, its own escape-sandbox flags | to write |
| SW3 system or user installation decided by a `CheckAuthorization` | not verified in its source | to write |
| SW15 and decision 6 (i): updates by a system timer with no session | not its job; its own automatic updates can be hidden | the timer is this document's in both options |
| SW4 background activity, SW5 features, SW6 failures in plain language | no | to write (package SWb) |
| SW2, SW7 to SW9 developer mode, Quadlet, Nix | no | to write (packages SWc, SWd) |
| SW10 confinement badges for Nix tools and user units | no | to write |
| SW16 the image's version, read-only | no | to write |
| SW14 untrusted input, Orca, gettext, Italian, right-to-left | GTK4 and libadwaita; Italian and Arabic translations; Orca not checked | to write |
| SH4 plain `gtk4-rs`, Rust | C | Rust |
| Fedora 43 RPM or verified Flathub build (decision 7, sixth criterion) | Flathub only, not sandboxed | own RPM |

**Options:**

- **(a) Athanor's Software as written:** every page of this document, SWa to SWd. Most code to own; the Flatpak catalogue, its search and its queue duplicate what Bazaar and GNOME Software already maintain.
- **(b) Bazaar for applications, plus an Athanor page** for what Bazaar does not do: background activity and features (SW4 to SW6), developer mode with Quadlet and Nix (SW2, SW7 to SW9), the confinement badges for Nix tools and user units (SW10) and the image's version (SW16). SWa shrinks to the update service of decision 6 (i), the preinstall files and Bazaar's configuration (curated page, blocklist, `hide-auto-update-options`); SWb to SWd stay.
- **(c) Bazaar alone.** Leaves the maintainer's request of 2026-09-30 (section 1.3) unmet for background activity, features and developer mode.

**Recommendation: (b),** with four conditions, each checked before the plan of SWa is written:

1. **Channel.** Preferably an RPM Athanor builds from the upstream tag (the pattern of `forge/specs`), unconfined as the user like Nautilus and declared in section 5, rather than the Flathub build, which SW3 calls not sandboxed and which needs a host override to read its configuration. **This does not hold on Fedora 43 today:** Bazaar `v0.9.7` requires GTK 4.22.1 or later, Fedora 43 ships `gtk4` 4.20.2 and Fedora 44 ships 4.22.5 (`mdapi.fedoraproject.org`, 2026-10-06). On a Fedora 43 base the choices are the Flathub build with its "not sandboxed" label stated, an older Bazaar tag that builds against GTK 4.20 (unchecked), or waiting for a Fedora 44 base. The maintainer chooses among them with option (b). **Decided (A2-28, #159):** option (b), and Bazaar waits for the Fedora 45 base: no Flathub build and no older tag.
2. **One updater.** `hide-auto-update-options: true`, and `bazaar-daemon` does not update: the system timer of decision 6 (i) remains the only updater of the system installation, and Bazaar's Updates page shows what it left pending.
3. **No distribution hooks.** The image configures none of Bazaar's hooks: they run shell snippets on user actions, which is code outside any review of this document.
4. **Scope budget** (`doc_shell.md` SH3). The Athanor page states its "why not upstream" (no upstream program covers SW4 to SW10 and SW16) and gets an owner named by the maintainer; Bazaar's maintenance owner on the image side is named too, because a pre-1.0 upstream that releases every two weeks moves under the image.

The maintainer chose (b) (A2-28, #159), and Bazaar waits for the Fedora 45 base. A revision of this document then rewrites SW1, SW3, SW13, SW15, SW17, section 3 and the acceptance items of package SWa for Bazaar plus the page; until then SW1 to SW17 stand as written.

### 9.5 Scope budget

Every new component of Athanor's own that does what an upstream project already does carries a written "why not upstream" and a maintenance owner before its first plan (`doc_shell.md` SH3, A2-14, #159). For this document: Software's application pages are weighed in 9.4; the features helper (SW12), the update service of decision 6 (i) and the developer pages have no upstream equivalent on this image and need only their owner.

### 9.6 Offline help (decided)

Maintainer decision A2-24 (#159): offline help ships in 1.0; the format is chosen in a short specification. Today the image has no help viewer, no user guide and no Yelp: on the maintainer's desktop (image 43.20261004.153) `/usr/share/help/C` holds only Orca's pages and `yelp` is not installed. This section is that short specification; the maintainer chose (a) (A2-30, #159).

- **(a) Yelp with Mallard pages of Athanor's own.** Fedora 43 packages `yelp` 49.2 (GTK4, libadwaita, WebKitGTK 6.0) and `yelp-xsl` (`mdapi.fedoraproject.org`, 2026-10-06). Athanor writes its guide as Mallard pages, English and Italian, under `/usr/share/help/<lang>/athanor/`, in a package of its own; the bar, Settings and the shield's sheet open `help:athanor/<page>`. Applications that ship their own help (Nautilus, GNOME Disks, Ptyxis) open theirs in the same viewer. `gnome-user-docs` is not installed: it describes GNOME Shell, and on this desktop it would mislead. Yelp follows the accent through libadwaita; its footprint on the image is not yet measured and is still to be measured (A2-30, #159).
- **(b) HTML pages opened in the browser.** No viewer to ship, but with Firefox as a Flatpak (9.3) the browser cannot read `/usr/share`, so the pages would need a local server or a copy into each user's home; applications' own Mallard help would still need Yelp.
- **(c) A help page inside Athanor's own surfaces.** A new own component under the scope budget (`doc_shell.md` SH3), for content Yelp already renders.
- **(d) Online only.** Excluded by A2-24.

**Decided: (a)** (A2-30, #159; the recommendation adopted). It reuses the viewer GNOME applications already target, needs no new own component and works offline in both languages; that Orca reads its pages is checked in its acceptance, not assumed. Its specification fixes the page list (first steps, the shield, updates, Software, developer mode, recovery), the package, the `help:` links each surface opens, and an acceptance case that opens every page with the network down. Until the package ships, no surface links to help.
