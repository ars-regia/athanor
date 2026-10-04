# Athanor Software: applications, background activity and developer tools

Status: Approved, rev 2 (2026-09-30): the maintainer accepted every recommendation of section 6, which now records the decisions. It turns the maintainer's request of 2026-09-30 into a specification: one application, working title **Software**, with which an average user never has to struggle and which also serves developers. Section 6 records the maintainer's decisions, with the options that were weighed. Section 4 lists what must be proven before the first plan is written.

## 1. Context

### 1.1 What binds this document

- `doc_shell.md`: the replacement rule and no facades (SH1, `doc_shell.md:36-42`); COSMIC applications, `cosmic-store` among them, are content and not dependencies (SH3, `doc_shell.md:71`); one crate per program on plain `gtk4-rs` (SH4, `doc_shell.md:75-76`); the Calmo tokens, WCAG AA contrast, high contrast and reduced motion (SH5, `doc_shell.md:84-96`); every string read from a system file is plain text, truncated, stripped of control and bidirectional characters (SH12, `doc_shell.md:149`); the test matrix, AT-SPI roles and names, gettext from the first commit (SH13, `doc_shell.md:172-182`).
- `doc_bar.md`: applications start behind a `wp_security_context_v1` socket in a transient unit of the user manager (BR2, `doc_bar.md:34-53`); untrusted strings are plain text (BR4, `doc_bar.md:79`); the shield and its sheet (BR6, `doc_bar.md:105-119`).
- `doc_update_trust.md`: system image updates, the state file and the two requests (UT6, UT7, UT11). **This document does not touch system image updates.** They stay in the shield and its sheet (`doc_bar.md`, BR3 and BR6) and in the notifier (`doc_update_trust.md`, UT11). Software shows the running version read-only and points at the shield (SW16).
- `doc_kernel_profile.md`: applications update through Flatpak with no interruption (class A, `doc_kernel_profile.md:479`); on the desktop class code in the home runs and is measured, and a quarantine prompt outside the kernel is bypassable, a stated residual risk (D23, `doc_kernel_profile.md:100`); code running as the user persists through autostart entries and `systemd --user` units, and a Flatpak application with home access can leave its sandbox (`doc_kernel_profile.md:715-722`).
- The project rule: no daemon or application outside a compartment or a MicroVM (`CLAUDE.md`, "Limiti inviolabili"). SW10 states where this document does not meet it.

### 1.2 What ships today

Checked in the repository at `27378de3`, and, where the repository cannot answer, on the maintainer's desktop on 2026-09-30. That desktop runs an Athanor image older than `iso-v0`: it carries no `os.athanor.update.policy`, so desktop findings may lag the branch.

**Applications.**

- `flatpak` is installed by the image build (`system/Containerfile:140`, `forge/config/packages.json:113`), on the base `quay.io/fedora-ostree-desktops/base-atomic:43` (`system/Containerfile:31`).
- `cosmic-store` ships (`forge/config/packages.json:156`) and is today the only graphical way to install an application.
- **No Flathub remote is configured.** The only remote on the desktop is `fedora` (system, OCI), which Fedora's `flatpak-add-fedora-repos.service` adds (enabled by Fedora's `90-default.preset:374`, desktop). The one script that adds Flathub and installs the `flatpaks` list of `forge/config/packages.json:203-208` is `system/scripts/provision_flatpak.sh:24-36`; the image copies it to `/scripts/` (`system/Containerfile:142`), and no unit, kickstart or script runs it. That list therefore installs nothing, as the `upstream_*` lists once did.
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

**Portals.** `xdg-desktop-portal` 1.20.4 is installed. The installed backends are `gtk` and `gnome-keyring`, and `athanor-portals.conf` routes every interface to them by name, with no backend for screen capture (`forge/specs/athanor-system-config/SOURCES/usr/share/xdg-desktop-portal/athanor-portals.conf`). **No installed backend implements `org.freedesktop.impl.portal.Background`** (desktop). The session announces `XDG_CURRENT_DESKTOP=Athanor:COSMIC` (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-session:18`).

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

**SW10. Confinement, stated.** The zero-trust rule is met by some workloads and not by others, and the interface says which.

| Workload                  | Confinement today                                                                | What escapes it                                                                              |
| ------------------------- | -------------------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| Flatpak application       | bubblewrap namespaces, portals, seccomp                                          | whatever its permissions grant; home or host access leaves the sandbox (SW3)                 |
| Quadlet container         | rootless user namespace, SELinux `container_t`, seccomp, its own cgroup          | mounts the user adds, host networking and the keys SW8 flags; the image itself is unverified |
| Nix tool                  | **none**: it runs as the user, unconfined, like any binary in the home under D23 | everything the user can do                                                                   |
| user unit written by hand | **none** by default                                                              | everything the user can do                                                                   |

This document does not claim that Nix tools or hand-written user units are compartmentalised. Their confinement is decision 3 (a): they carry a "Not isolated: runs with all your rights" badge and exist only in developer mode.

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
   - **Decided:** the table as it stands, with `backup` bound to whichever snapshot mechanism the backup rewrite keeps, and `smart-cards` shown only when a reader is present. Remote login is **off** by default on new installs: the kickstart stops enabling `sshd` (`system/athanor-install.ks:37-38`) and stops opening the SSH port in the same package that ships the feature switch (SWb), so that turning it on never needs a terminal. Existing installs keep their state.

3. **Confinement of Nix and Quadlet workloads.**
   - (a) **Declared exception:** Nix tools and hand-written units stay unconfined, only in developer mode, with the badge of SW10, and the exception is written into the project rule by the maintainer (section 7).
   - (b) **Per-tool sandbox:** run every Nix tool through a bubblewrap or Landlock wrapper. A generic profile either breaks developer tools, which need the home and the network, or restricts nothing.
   - (c) **A container for untrusted tools:** Nix inside a `toolbox` container (already on the image) or the dev VM, with only the profile's result exported. It isolates, at the cost of friction.
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
   - **Decided:** (b) and (i). Without Flathub the Explore page would be close to empty, and a system timer is the only one of the three that updates a machine nobody is logged in to, without widening polkit. The `flatpaks` list is deleted: a machine that installs Chrome and Spotify unasked is not "for everyone".

## 7. Changes to other documents

Each change lands with the package named beside it, not with this document.

- `doc_shell.md`, SH3 (`doc_shell.md:71`): `cosmic-store` leaves the image at the switch of package SWa (decision 5). With SWa.
- `doc_shell.md`, section 3, "Later stages": a pointer to this document as an application track after stage 2. With the first plan (package S).
- `experimental/EXEMPT:18-22`: the comment on `athanor-store-rs` is removed with the crate. With SWa.
- `CLAUDE.md`, "Limiti inviolabili": the maintainer writes the exception of decision 3 (a): Nix tools and hand-written user units run unconfined, only in developer mode and marked as such. Before SWc.
- `forge/config/packages.json`: the `flatpaks` list and `system/scripts/provision_flatpak.sh` are deleted (decision 6). With SWa.
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
