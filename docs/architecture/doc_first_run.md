# Athanor first run

Status: **revision 1 draft, 2026-10-05: the maintainer's decisions applied; text not yet reviewed.** It specifies what a person meets the first time an installed Athanor machine starts: the session that runs before any account exists, its screens (language, keyboard, network, time zone, privacy and updates, appearance, the account), accessibility from the first frame, what the program may change as root and how each change is authorised, how the choices reach the person's first session, the tests and the order of construction. It does not design the installer's disk screens, the Settings application's pages, or the internals of the preferences other specifications own; where it needs one of them it names the interface and the owner. The maintainer's decisions of 2026-10-05 are in section 6 with their reasons; the rules state the choices taken.

**Amendment of 2026-10-06** (maintainer decisions A2-5 and A2-22 of 2026-10-05, specification audit 2, #150): security updates apply at the next shutdown or restart the person starts (section 1, FR12, decision 4), and the installer of release 1.0 is Anaconda with its web interface (section 1, spike F2). The earlier text is kept and marked where it is amended. *Amended on 2026-10-06 (A2-27, as amended by A2-35 (#131, #145)):* the MOK enrolment is prepared by the installer on its last page, before the first reboot: the installer generates a one-time password, prepares the request and shows the password, and MokManager asks for it at the first boot (`doc_kernel_profile.md`, section 4). First run then checks that the enrolment succeeded and, if it did not, explains how to redo it and can prepare the request again (FR5, FR7); the `mokutil` command line stays available. The check changes no disk. `athanor-tpm-luks-seal` is disabled until 1.1, and 1.0 unlocks with the passphrase.

## 1. Context

- **What binds this document.**
  - `doc_shell.md`: of COSMIC only cosmic-comp stays, and `athanor-compositor-client` is the only crate that may know COSMIC (SH2, SH3, SH4); logic in modules with no GTK type (SH4); the identity "Calmo" (SH5); the test matrix of 12 cases (SH13). The default preset is chosen once per user at the first session (SH10), so first run does not choose it.
  - `doc_shell_standard.md`: accessibility and languages (ST7), the reference machine (ST4) and its bench (ST9). First run is not a surface that replaces a COSMIC one, so the gate of ST2 does not apply; ST7's checks do.
  - `doc_visual_language.md` (revision 1, PR #119): the mark appears in the greeter, the lock screen, first run and About (VL8); first-run headings use `title-1`, 181 % (`doc_visual_language.md:93`); the appearance schema `org.athanor.desktop.appearance` and the one function in `athanor-style` that applies it (VL4).
  - `doc_languages.md` (revision 1): the interface first run calls (LN16), the stores and writers (LN15), the system locale only through localed (LN5), the user's languages in AccountsService (LN6), the region key read by `athanor-session` at login (LN7), the keyboard store (LN9), the greeter in the system locale and layout (LN10).
  - `doc_accessibility.md` (revision 1): the preferences and their keys (AX2), `athanor-a11y` and Orca as user units (AX3), the reader gate (AX5), the greeter's accessibility menu, speech before login and the hand-off to the session (AX13), the shortcuts Super+Alt+S and Super+Alt+A (decision 8).
  - `doc_lock_and_prompts.md` (revision 1): the root helper per connection, admitted by cgroup through a pidfd, as the model for a small privileged surface (LP8, decision D1).
  - `doc_software.md` (revision 3, PR #117): remote login (SSH) off on new installs (decision 2); Flathub as a system remote shipped by the image, and a system timer for application updates (decision 6).
  - `doc_update_trust.md` and `doc_shell.md` SH11: system updates are checked and downloaded by a system timer, never applied without the person's confirmation, and not downloaded on a metered connection (UT1, UT11, UT12). *Amended on 2026-10-06 (A2-5, #150):* that holds for feature updates; a security update applies at the next shutdown or restart the person starts, with a notice and a one-step way back (SH11, UT13).
- **The register.** `shell-features.md:380` holds F-settings-33, "First-run setup wizard", `missing`, with the note "not verified: system/athanor-oobe exists in the tree". This document is that entry's specification.
- **How a machine is installed today** (repository at `visual-language-spec`, read on 2026-10-05).
  - The ISO is built by bootc-image-builder's `anaconda-iso` from `system/disk_config/iso.toml` (`system/scripts/build_bib.sh:11`, `forge/scripts/build_iso.sh:20`). Its kickstart sets the graphical target and fixes `/etc/fstab`; it declares no user, and enables Anaconda's Storage, Runtime, Network, Security, Services, Users and Timezone modules (`iso.toml:70-77`). Anaconda therefore asks for the disk, the time zone and the account.
  - In Anaconda's `fedora-43` branch the user spoke runs only when the Users module is available, and is mandatory only while no administrator is requested (`pyanaconda/ui/gui/spokes/user.py`, `should_run` and `mandatory`, raw.githubusercontent.com, 2026-10-05). Disabling the module removes the screen.
  - **Amendment of 2026-10-06** (maintainer decision A2-22 of 2026-10-05, #150). The installer of release 1.0 is Anaconda with its web interface, and the x86-64-v3, UEFI and disk checks run in its kickstart (`doc_kernel_profile.md`, D3, D14, P4b). The bullet above reads a screen of Anaconda's GTK interface (`pyanaconda/ui/gui/spokes`); whether the web interface shows an account or a time zone screen, and whether disabling the Users and Timezone modules removes them there, has not been checked. Spike F2 checks it on the web interface (section 4).
  - `system/athanor-install.ks` is the kickstart of the manual ISO build documented in `system/README.md:128`; the workflow builds use `iso.toml`. It declares no user and locks `root` (`:31`), enables `systemd-homed` (`:38`; `sshd` left it with decision 2 of `doc_software.md`), and says that systemd-homed encrypts the user's home (`:40-42`), although Anaconda creates classic accounts; see section 4, doubt 7.
  - The raw disk image (`system/disk_config/disk.toml:5-9`) carries no user on purpose, so a machine started from it today has no account and no way to make one.
  - The test and development kickstarts declare their user (`forge/test/iso/collaudo.ks:49`, `scripts/devvm/devvm.ks:33`).
  - Images built before 2026-10-06 enable `systemd-homed.service` and authselect's `with-systemd-homed`, but Anaconda creates classic accounts in `/etc/passwd` (unverified on an installed machine; spike F2). Decision 0045 disables homed until a homed specification with a migration exists.
- **The greeter.**
  - greetd 0.10.3 runs `athanor-greeter-session` as the `greetd` user on VT 1 (`forge/specs/athanor-system-config/SOURCES/usr/share/athanor-system-config/greetd.toml`), which runs cosmic-comp with `athanor-greeter-client` as its only client. The client is confined from outside by bubblewrap: no network, `/usr` and `/etc` read-only, a filtered system bus that forwards only logind's `Suspend`, `Reboot` and `PowerOff`, and a filtered session bus for accessibility (`SOURCES/usr/libexec/athanor-greeter-client`). That model is reused here (FR4).
  - The greeter greets the first account in `/etc/passwd` whose uid is 1000 to 65533 and whose shell ends in `bash`, `zsh` or `fish`, and falls back to the name `athanor` when there is none (`athanor-greeter-ui-1.0.0/src/auth.rs:114-139`). A machine with no account therefore shows a card for an account that does not exist.
- **The prototype.** `system/athanor-oobe` (relm4, a workspace member, not packaged) runs `localectl`, writes `/etc/locale.conf` and `/etc/vconsole.conf` directly when that fails, and writes `telemetry_opt_in`, `crash_reports_opt_in` and `ebpf_ai_opt_in` to `/etc/athanor/oobe.json` (`main.rs:38-130`); its telemetry switch is on by default (`main.rs:624`). No program reads that file (`rg` over the tree, 2026-10-05): its switches are controls that do nothing, which SH1 calls a defect. `doc_platform_experience.md:24-33` describes a first boot inside the greeter that generates an X25519 mesh identity and logs in to Cloudflare Zero Trust; neither exists. Done: ADR-0073 deleted it from the tree.
- **Facts verified on the maintainer's desktop on 2026-10-05** (Athanor OS 43, `rpm -q`, files under `/usr/share`).
  - Installed: accountsservice 23.13.9-9.fc43, systemd 258.11-1.fc43, greetd 0.10.3-6.fc43, polkit 126-6.fc43.2, NetworkManager 1.54.3-3.fc43, geoclue2 2.8.2-1.fc43, chrony 4.8-3.fc43, xdg-desktop-portal 1.20.4-1.fc43, xdg-dbus-proxy 0.1.8, bubblewrap 0.12.0, libpwquality 1.4.5-14.fc43, cracklib-dicts 2.10.3, libxcrypt 4.5.2, libxkbcommon 1.11.0, xkeyboard-config 2.46, tzdata 2026c, fedora-third-party 0.10-15.fc43. Not installed: gnome-initial-setup, initial-setup, plasma-setup, abrt.
  - polkit defaults (`/usr/share/polkit-1/actions`): `org.freedesktop.locale1.set-locale` and `set-keyboard`, `org.freedesktop.timedate1.set-timezone` and `set-ntp`, `org.freedesktop.hostname1.set-static-hostname` and `org.freedesktop.home1.create-home` are `auth_admin_keep` for every subject; `org.freedesktop.accounts.user-administration` is `auth_admin_keep` for an active session and `auth_admin` otherwise; `org.freedesktop.NetworkManager.settings.modify.system` and `network-control` are `yes` for an active session.
  - Rules in `/usr/share/polkit-1/rules.d`: `10-athanor-wheel-admin.rules` makes `wheel` the administrators (package `athanor-system-tweaks-1.0.0-5`); `org.fedoraproject.thirdparty.rules` lets an active local member of `wheel` opt out of third-party repositories.
  - AccountsService's `CreateUser(name, fullname, accountType)` creates the account through `/usr/sbin/useradd`; an administrator account joins `wheel` (`strings /usr/libexec/accounts-daemon`). `SetPassword(password, hint)` takes "the crypted password" (`/usr/share/dbus-1/interfaces/org.freedesktop.Accounts.User.xml:582-595`); `SetLanguages` and `DeleteUser` exist (`:153`, `org.freedesktop.Accounts.xml:165`).
  - `/etc/login.defs`: `UID_MIN 1000`, `UID_MAX 60000`, `ENCRYPT_METHOD YESCRYPT`. `/etc/security/pwquality.conf` sets nothing; since 2026-10-07 the image adds `/etc/security/pwquality.conf.d/50-athanor.conf` with `minlen = 12`, and libpwquality's built-in limits apply otherwise.
  - `/usr/lib/os-release` sets `DEFAULT_HOSTNAME="athanor"`.
  - Contacts the image makes by itself: NetworkManager's connectivity check fetches `http://fedoraproject.org/static/hotspot.txt` every 300 s (`/usr/lib/NetworkManager/conf.d/20-connectivity-fedora.conf`, package NetworkManager-config-connectivity-fedora); chrony uses `pool 2.fedora.pool.ntp.org` (`/etc/chrony.conf`); Wi-Fi uses a stable per-network random MAC address (`22-wifi-mac-addr.conf`, `wifi.cloned-mac-address=stable-ssid`).
  - Location: `org.gnome.system.location enabled` defaults to `false` (gsettings-desktop-schemas 49.1). geoclue's Wi-Fi source queries an Ichnaea service whose built-in default is `https://api.beacondb.net/v1/geolocate` (`/etc/geoclue/geoclue.conf:73-78`); geoclue's agent whitelist names no Athanor program (`geoclue.conf`, `[agent]`).
  - systemd 258 (`systemd.service(5)`): an `ExecCondition=` command that exits 1 to 254 skips the unit without marking it failed; a `Type=oneshot` unit has no start timeout by default and accepts `Restart=on-failure` but not `always`.
  - polkit 126 (`polkit(8)`): rules files sort by basename across `/etc` and `/usr/share`, the first rule to return a result wins; `polkit.spawn()` runs a helper as the `polkitd` user and blocks every other authorization check while it runs.
- **How others do it** (checked 2026-10-05).
  - GNOME Initial Setup runs before any account exists as its own system user, which a polkit rule lets change, without a password and from a local session, every action under the prefixes `org.freedesktop.hostname1.`, `NetworkManager.`, `locale1.`, `accounts.`, `timedate1.`, `realmd.`, `com.endlessm.ParentalControls.`, `org.fedoraproject.thirdparty.`, and `org.freedesktop.home1.passwd-home` (`data/20-gnome-initial-setup.rules.in`, gitlab.gnome.org, main). Its `data/` directory carries a copy worker and a first-login service that carry its choices into the new user's session (`gnome-initial-setup-copy-worker.service.in`, `gnome-initial-setup-first-login.service.in`).
  - Plasma 6.6 introduced Plasma Setup, "the new first-run wizard for Plasma", which "creates and configures user accounts separately from the installation process", so that a machine can be preinstalled or refurbished and handed over (https://kde.org/announcements/plasma/6/6.6.0/).
  - macOS asks during initial setup whether to run the VoiceOver screen reader (American Foundation for the Blind, https://www.afb.org/blindness-and-low-vision/using-technology/using-computer/part-ii-experienced-computer-user-new-3); the exact prompt and its timing are unverified.
  - A compositor run as a system service with a PAM session on a VT is the documented pattern of Weston's `weston.service` (`PAMName=`, `TTYPath=`, `UtmpIdentifier=`; openembedded-core `meta/recipes-graphics/wayland/weston-init/weston.service`, and the 2017 wayland-devel patch "doc/systemd: system service example"). It is unverified for cosmic-comp (spike F1).

## 2. Decisions

**FR1. What first run is, and when it runs.** First run is the program that turns an installed machine with no account into a machine with one administrator, set in the person's language, keyboard and time zone. It runs once per machine, before the greeter, and never again.

- **The condition is derived, not stored.** First run is pending while no account with a uid from `UID_MIN` to `UID_MAX` of `/etc/login.defs` exists, read through NSS (`getpwent`), so accounts of every source count: `/etc/passwd`, systemd-homed through `nss-systemd`, and any directory service. No stamp file says "done", so nothing can be left behind half-written, and a machine whose installer, kickstart or disk-image recipe already made an account never sees first run. The test and development kickstarts keep working unchanged.
- **The split with the installer** (decision 1): the installer keeps what only it can do (its own language and keyboard, the disk, encryption) and leaves the account and the time zone to first run.
- **Per-user setup is not designed here.** An account created later in Settings, or by an installer, gets no first-run screens (FR19, decision 8).
- **`system/athanor-oobe` leaves the workspace** in the first construction step (decision 9). Nothing of it is reused: it writes `/etc` directly and stores switches no program reads. Done: ADR-0073 deleted it from the tree.

**FR2. Programs, crates and units.**

| Crate                           | Units                                                                                                                      | Role                                                                                                                                                                                                         |
| ------------------------------- | -------------------------------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ |
| `forge/specs/athanor-first-run` | `athanor-first-run.service` (system); `athanor-first-run-account.socket` and `athanor-first-run-account@.service` (system); _(Proposal)_ `athanor-first-run-mok.socket` and `athanor-first-run-mok@.service` (system, FR5) | the screens (`athanor-first-run`), the pending check (`athanor-first-run pending`), the account helper (`athanor-first-run account-helper`), _(Proposal)_ the enrolment helper (`athanor-first-run mok-helper`, FR5), the hand-off at first login (`athanor-first-run apply-handoff`) |

- **One crate, one binary with subcommands.** The screens, the helper and the hand-off share the hand-off format, the validation of user names, real names and language codes, and the pending check; one crate keeps one copy. The helper and the hand-off never initialise GTK: their subcommands return before any GTK call, and the helper's code path links no GTK symbol into the work it does (checked by its tests running without a display).
- **Logic in GTK-free modules** (SH4): `pending`, `handoff`, `names` (derivation and validation of the user name), `password` (quality and hashing), `zones` (time-zone suggestions), `layouts` (the Latin check of FR9), `steps` (the screen order and which screens apply). The interface only draws.
- **libadwaita.** First run is an application-like sequence of pages, not a shell surface, so it uses libadwaita 1.8.8 (`AdwNavigationView`, `AdwStatusPage`, `AdwPreferencesGroup`) with the visual language's application materials untouched (VL2), as Athanor's own applications do.
- **Its own system user,** `athanor-firstrun`, declared by `/usr/lib/sysusers.d/athanor-first-run.conf`: `u athanor-firstrun - "Athanor first run" /var/lib/athanor-first-run /usr/sbin/nologin`. The account has no password and no login shell; only systemd starts processes under it, and only through `athanor-first-run.service`.
- **Translations** go through `athanor-i18n`, as LN16 says for first run's own text, with its own gettext domain and the catalogs of LN4.

**FR3. The session: a system unit that runs before the greeter.** First run is not a greetd session. It runs as a system service that holds VT 1 until it ends, and greetd starts after it.

- **Why not greetd.** greetd's `initial_session` runs once at start and then falls back to the greeter, so a crash of first run would leave the greeter showing an account that does not exist (section 1). Its `default_session` would restart first run, but greetd reads its configuration once, so after the account exists greetd itself would have to be restarted by something privileged. A unit ordered before greetd needs no change to greetd or its configuration, restarts on a crash, and ends by exiting.
- **The unit**, sketched:

  ```
  [Unit]
  Description=Athanor first run
  After=systemd-user-sessions.service plymouth-quit-wait.service
  Before=greetd.service
  Conflicts=getty@tty1.service

  [Service]
  Type=oneshot
  ExecCondition=/usr/libexec/athanor-first-run pending
  ExecStart=/usr/libexec/athanor-first-run-session
  User=athanor-firstrun
  PAMName=athanor-first-run
  TTYPath=/dev/tty1
  StandardInput=tty-fail
  UtmpIdentifier=tty1
  StateDirectory=athanor-first-run
  StateDirectoryMode=0755
  Restart=on-failure
  RestartSec=1s

  [Install]
  WantedBy=graphical.target
  ```

  - `pending` exits 0 while no account exists and 1 otherwise, so on every later boot the unit is skipped without failing (section 1, `systemd.service(5)`).
  - `Type=oneshot` with no start timeout keeps greetd waiting for as long as the person needs; `Restart=on-failure` brings first run back after a crash.
  - A drop-in `greetd.service.d/20-athanor-first-run.conf` adds `After=athanor-first-run.service`, so the greeter starts only when first run has ended or was skipped. greetd keeps its own `Wants`; the existing `10-athanor-wantedby.conf` is unchanged.

- **The PAM service** `/usr/lib/pam.d/athanor-first-run` opens a logind session for `athanor-firstrun` on seat0 with a user manager and a session bus, which accessibility needs (FR16): `account required pam_permit.so`, `session required pam_loginuid.so`, `session required pam_systemd.so`, and nothing that authenticates, because systemd does not call the `auth` phase for `PAMName=`. The session must be local and active for polkit, and must give `athanor-firstrun` its user manager; spike F1 verifies both and sets the session class.
- **`athanor-first-run-session`** is the twin of `athanor-greeter-session`: it parses `/etc/locale.conf` the same way, without sourcing it, and runs `cosmic-comp --no-xwayland /usr/libexec/athanor-first-run-client` under `systemd-cat -t athanor-first-run`. cosmic-comp exits when its client exits, and the unit with it.
- **The end.** When the account exists and the hand-off is written (FR15), the client exits 0, the unit becomes inactive, and greetd starts the greeter on VT 1, which greets the new account (section 1, `auth.rs:114-139`).
- **Power.** The power button and the screens' power menu suspend, restart or shut down through logind, as the greeter does. A restart before the account exists brings first run back, with the system settings already applied preselected (FR18).

**FR4. Confinement of the interface.** The program that draws the screens and reads the password is confined from outside, before it executes, as the greeter's client is.

- **`athanor-first-run-client`** follows `athanor-greeter-client` line for line, with these differences:
  - the state directory `/var/lib/athanor-first-run` is bound read-write, the only writable path outside the private tmpfs mounts;
  - the account helper's socket `/run/athanor/first-run-account.socket` is bound in, and greetd's is not;
  - _(Proposal)_ the enrolment helper's socket `/run/athanor/first-run-mok.socket` is bound in as well (FR5);
  - the filtered system bus forwards what FR5 lists and nothing else;
  - the filtered session bus is the greeter's: its own application name, `org.a11y.Bus`, and the AT-SPI socket alone (AX5, AX13).
- **No network.** bubblewrap's `--unshare-all` gives the client an empty network namespace. NetworkManager joins networks; the client only asks it to.
- **No core, no tracing.** At start, before any thread exists, the client sets `PR_SET_DUMPABLE` 0 and `RLIMIT_CORE` 0, as the greeter does (`athanor-greeter-ui-1.0.0/src/sandbox.rs:32-41`). The password is held in `Zeroizing` buffers from the entry to the helper's socket and cleared after the reply (FR14).
- **No polkit agent.** The client registers none. Every call it makes is either allowed by a default or by FR5's rule, or refused; a refusal is shown as an error, never as a password prompt drawn by something else.
- **Untrusted input.** The only text the client receives from outside the machine is the names of nearby Wi-Fi networks, through NetworkManager. They are shown as plain labels, never as markup, and SSIDs that are not valid UTF-8 are shown with replacement characters.

**FR5. What first run may change as root, and how each change is authorised.** Every privileged change goes through a system service, under that service's polkit action. First run writes no file outside its state directory.

| Change                               | Service and call                                               | Authorisation                                                                                        |
| ------------------------------------ | -------------------------------------------------------------- | ---------------------------------------------------------------------------------------------------- |
| System language                      | localed `SetLocale` (LN5, LN16)                                | `30-athanor-first-run.rules` (below)                                                                 |
| System keyboard                      | localed `SetX11Keyboard`, `convert` true (LN5, LN16)           | `30-athanor-first-run.rules`                                                                         |
| Time zone, network time              | timedated `SetTimezone`, `SetNTP`                              | `30-athanor-first-run.rules`                                                                         |
| A Wi-Fi network for the whole system | NetworkManager `AddAndActivateConnection`                      | NetworkManager's defaults for an active session (`settings.modify.system`, `network-control`: `yes`) |
| The account                          | the account helper of FR6, which calls AccountsService as root | the helper's own checks (FR6, decision 3)                                                            |
| Suspend, restart, shut down          | logind `Suspend`, `Reboot`, `PowerOff`                         | logind's defaults, as at the greeter                                                                 |
| The Secure Boot enrolment check      | the enrolment helper (below): `mokutil --sb-state`, `--test-key` | the helper's own checks, as FR6's (A2-35, #131, #145)                                                |
| A new enrolment request              | the enrolment helper: `mokutil --import`, `--hash-file` (root) | the helper's own checks, as FR6's; no polkit action, so the rule keeps its four actions              |

- **The rule** `/usr/share/polkit-1/rules.d/30-athanor-first-run.rules`, shipped by `athanor-first-run`, returns `polkit.Result.YES` when `subject.user` is `athanor-firstrun`, `subject.local` and `subject.active` are true, and `action.id` is exactly one of `org.freedesktop.locale1.set-locale`, `org.freedesktop.locale1.set-keyboard`, `org.freedesktop.timedate1.set-timezone`, `org.freedesktop.timedate1.set-ntp`; it returns nothing otherwise. It sorts after `10-athanor-wheel-admin.rules` and before `40-athanor-locale1.rules`, as LN5 requires for a program that needs localed's actions without a password. The maintainer approved it on 2026-10-05 with exactly these four actions, this user, local and active only (decision 7).
- **The enrolment helper** _(Proposal, A2-35, #131, #145)_. `athanor-first-run-mok.socket`, listening on `/run/athanor/first-run-mok.socket`, and `athanor-first-run-mok@.service`, built as the account helper of FR6: the same activation, the same peer check (the cgroup of `athanor-first-run.service` and the uid `athanor-firstrun`), only while first run is pending, and FR6's sandbox with write access to `/sys/firmware/efi/efivars` alone. It answers two requests. `status` reads the Secure Boot state (`mokutil --sb-state`) and whether the project certificate shipped in the image is enrolled (`mokutil --test-key` on its DER file). `prepare` generates a new one-time password, writes its SHA-512 crypt hash to a private temporary file, runs `mokutil --timeout -1` and `mokutil --import` with `--hash-file`, and returns the password for the screen to show; the password is held and cleared as FR6 holds the account password, and never logged. `mokutil --import` writes the EFI variables that shim reads at the next boot, so it needs root; the helper, not a polkit rule, keeps that to one request per first run. Its socket is bound into the client beside the account helper's (FR4).
- **Narrower than GNOME's.** No prefix match, no hostname, no realmd, no third-party repositories, no homed, and no `org.freedesktop.accounts.*`: the account is not created under a standing grant (decision 3).
- **The grant cannot outlive first run in practice:** only `athanor-first-run.service` runs processes as `athanor-firstrun`, and that unit is skipped from the moment an account exists (FR1).
- **The filtered system bus** forwards, by name and method: localed's `SetLocale`, `SetX11Keyboard` and its properties; timedated's `SetTimezone`, `SetNTP`, `ListTimezones` and its properties; NetworkManager's whole interface (`--talk`), because joining a network uses many of its objects and its defaults already grant an active session the same; logind's three power calls. AccountsService, hostnamed, systemd, udisks, Flatpak and every other service stay out of reach.
- **Not written by first run:** the static hostname, which stays the image's `athanor` (`os-release` `DEFAULT_HOSTNAME`), so that no person's name is announced to every network the machine joins through DHCP and mDNS; the firewall; `sshd` (off on new installs, `doc_software.md` decision 2); Flatpak remotes (`doc_software.md` decision 6); disk encryption and the TPM; anything under `/etc` directly.

**FR6. The account helper** (decision 3). The one change that creates an administrator goes through a root helper per connection that can do exactly one thing, once. The account is a classic account through AccountsService (decision 2).

- **Approval before code.** The helper takes a password, checks its quality and hashes it as root: it touches authentication. Its implementation plan (the crates or FFI for libpwquality and libxcrypt, the handling of the password buffer, the socket protocol and the sandbox) is approved by the maintainer before any code is written, in FR21 step 3.

- **Activation.** `athanor-first-run-account.socket` listens on `/run/athanor/first-run-account.socket` with `Accept=yes`, `SocketUser=root`, `SocketGroup=athanor-firstrun`, `SocketMode=0660`; each connection starts one `athanor-first-run-account@.service`, which handles one request and exits. The socket is pulled in only by `athanor-first-run.service` (`Wants=` and `After=` on the socket), never by a target, so nothing listens on a machine where first run was skipped. A socket unit cannot express "no account exists" as a condition, so the helper checks it itself (below).
- **Who may ask:** a peer whose process sits in the cgroup `/system.slice/athanor-first-run.service`, matched exactly. The helper resolves the peer through `SO_PEERPIDFD`, reads its cgroup, and checks after the read that the pidfd still names a live process, as `athanor-unlockd` does (LP8). The peer's uid must be `athanor-firstrun`.
- **When:** only while first run is pending (FR1), checked at the start of the request and again immediately before `CreateUser`. Otherwise it answers `ERR not-pending` and does nothing.
- **What it accepts:** one request of at most 4096 bytes, as four NUL-separated fields: the user name, the real name, the languages (a comma-separated list), the password. A longer request, a missing field or a field that fails its check ends the connection with `ERR invalid <field>`.
  - User name: `^[a-z_][a-z0-9_-]{0,31}$`, not the name of an existing account of any uid, and not `root`, `athanor` or `athanor-firstrun`.
  - Real name: valid UTF-8, 1 to 255 bytes, no control characters and no `:` (a field separator of `/etc/passwd`).
  - Languages: each entry matches `[A-Za-z0-9_.@-]+`, as LN6 checks them; at most 8 entries.
  - Password: 1 to 1024 bytes, and accepted by libpwquality's `pwquality_check` with the system's configuration and the user name as context (FR14). The helper repeats the check so that a defect in the interface cannot create a weaker password than the screen promised.
- **What it does,** in this order, each call to AccountsService on the system bus as root:
  1. `CreateUser(name, real_name, 1)`: an administrator, which AccountsService puts in `wheel`.
  2. `SetLanguages(languages)` on the new user (LN6).
  3. `SetPassword(hash, "")`, where `hash` is computed in the helper with libxcrypt: `crypt_gensalt_rn` with a null prefix, which follows the system default (`ENCRYPT_METHOD YESCRYPT`, section 1), then `crypt_rn`. The plain password is cleared from memory right after the call, and never logged.
  4. On a failure at step 2 or 3: `DeleteUser(uid, true)`, so that the machine is still pending and first run can try again; the answer is `ERR <step>`.
  5. On success: `OK <uid>`.
- **Confinement:** the sandbox of `athanor-unlockd@.service` (LP8), without its write path: `ProtectSystem=strict`, `ProtectHome=yes`, `PrivateNetwork=yes`, `RestrictAddressFamilies=AF_UNIX`, `NoNewPrivileges=yes`, `SystemCallFilter=@system-service`, `MemoryMax=32M`, `RuntimeMaxSec=60`, no `ReadWritePaths=`. AccountsService does the writes, under its own unit.
- **Why a helper and not a polkit rule** (decision 3): a standing grant of `org.freedesktop.accounts.user-administration` lets the holder create any number of administrators; the helper creates one, only while none exists, and only for the first-run session.

**FR7. The screens and their order.**

| #   | Screen              | Shown when                                                        | Writes                                 |
| --- | ------------------- | ----------------------------------------------------------------- | -------------------------------------- |
| 1   | Language            | always                                                            | system locale (FR8)                    |
| 2   | Keyboard            | always                                                            | system keyboard (FR9)                  |
| 3   | Network             | no connection reports full connectivity and a Wi-Fi device exists | a system Wi-Fi connection (FR10)       |
| 4   | Time zone           | always                                                            | time zone, network time (FR11)         |
| 5   | Privacy and updates | always                                                            | the hand-off: location (FR12)          |
| 6   | Appearance          | always (decision 5)                                               | the hand-off: appearance (FR13)        |
| 7   | Your account        | always                                                            | the account, through the helper (FR14) |
| 8   | Secure Boot key     | the enrolment check of FR5 reports the certificate not enrolled   | a new enrolment request (FR5)          |
| 9   | Ready               | always                                                            | the hand-off file, then exit (FR15)    |

- **Back and forward.** Every screen but the first has Back; Back never undoes a write already made, it shows the screen again with the current value selected. The account is created only on the last step, so every earlier choice can still change.
- **The header** of every screen carries the accessibility menu button at the end edge (the start edge under right-to-left text) and the power menu, always visible (FR16).
- **The first screen** shows the mark (VL8) above the title "Welcome", in `title-1`.
- **The Secure Boot key screen** (A2-35, #131, #145; _(Proposal)_ for its wording and placement). First run asks the enrolment helper for `status` at start. On a machine with Secure Boot on, a missing certificate stops the boot at MokManager (`doc_kernel_profile.md`, section 4), so in practice the screen appears on a machine whose Secure Boot is off or whose enrolment was declined. It says that the enrolment did not succeed and that the machine runs in degraded mode without it, and how to redo it: "Prepare again" asks the helper for a new request and shows the new one-time password, to type in MokManager at the next boot, with the advice to turn Secure Boot on in the firmware settings when it is off; the `mokutil` command line is named for later.
- **No screen is skippable except Network and Secure Boot key,** whose secondary buttons are "Set up later" and "Not now". Every other screen has a preselected value, so Next always works.

**FR8. Language** (LN16, step 1).

- **The list** is `athanor_i18n::offered_languages()`: complete languages first, then partial ones with the sentence "Athanor's own surfaces stay in English" (LN4). Each entry shows the language's own name, and in brackets its name in the current language when the two differ. A search field filters by either name.
- **Preselected:** the language of `/etc/locale.conf` when it is not `C` or `C.UTF-8`; otherwise English (United States).
- **On Next,** first run calls `SetLocale` with `LANG=<locale>` and, for a language with more than one entry in its fallback chain, `LANGUAGE`.
- **Its own text switches at once.** After `SetLocale` returns, the client replaces itself with `execve` of the same binary with the new `LANG` and `LANGUAGE` and the argument `--at keyboard`. Re-executing changes the language of libadwaita's and GTK's own strings and the text direction together, which a call to `setlocale` in a running multi-threaded GTK process cannot do safely. The accessibility state lives in the user's keys and in Orca's unit, so it is not lost across the re-execution.
- **The greeter and the console** follow at their next start (LN15).

**FR9. Keyboard** (LN16, step 1).

- **The list** is `athanor_compositor_client::keyboard::available()` (LN9): layouts with their variants and translated descriptions. The suggested layout is the one of the chosen language's main country, as LN16 states, and is preselected; a search field filters the rest.
- **A test field** under the list types with the selected layout. The client sets it live through `athanor_compositor_client` (`set_keyboard_group` on a keymap holding the candidate), so what the person types is what the greeter will receive.
- **A layout that cannot type a user name.** User names are ASCII (FR6). The `layouts` module compiles the chosen layout with libxkbcommon and checks that every letter `a` to `z` is reachable on its first group at level 1 or 2. When one is not, the system keyboard gets the layout followed by `us`, and the screen says: "English (US) is added so that you can type your user name. Switch with the button on the login screen." The greeter's switcher of LN10 appears with two layouts.
- **On Next,** first run calls `SetX11Keyboard(layouts, model, variants, options, convert=true, interactive=false)` with the model and options left as they are.
- **The disk passphrase.** When the root or home file system is encrypted and the new layout differs from the one in effect, the screen says that the disk's passphrase prompt at startup may still use the layout of the installer, until spike F3 settles what the prompt uses.
- **Input methods** (IBus engines) are not offered here: they are per user and belong to Settings (LN11, LN15).

**FR10. Network.**

- **Shown** only when NetworkManager's `Connectivity` is not `FULL` (4) and a Wi-Fi device exists. A wired machine, or one the installer already connected, never sees it.
- **The list** shows nearby networks by name, signal and whether they are protected, refreshed by NetworkManager's scan. Open networks and WPA2 or WPA3 Personal networks are joined from the list with their password.
- **The connection is the system's,** without `connection.permissions`, with the secret stored by NetworkManager itself (`psk-flags` 0), so that the update timer and the greeter have the network before anyone logs in, as an installer-made connection has. No secret agent is involved; first run registers none.
- **"Set up later"** goes on. Hidden networks, enterprise (802.1X) networks and captive portals are joined later from the session (`doc_control_center.md`, CC5), because first run has no browser and no certificate store; the screen says so in one line.
- **Without a network,** first run completes: network time waits for a network, and no screen needs one.

**FR11. Time zone and clock.**

- **The list** is timedated's `ListTimezones`, shown by city and region with the current local time beside each, and a search field. City names are the time-zone database's English names in release 1 (FR19).
- **Preselected,** in this order: the zone in effect when it is not `UTC`; else the only zone of the chosen language's country in tzdata's `zone1970.tab`; else the zones of that country listed first, none selected, and Next disabled until one is.
- **No location lookup.** First run never asks a network service where the machine is: the location switch comes later (FR12) and is off until the person turns it on.
- **"Set the time automatically"** is a switch, on by default, that calls `SetNTP`; the line under it says that the time comes from `2.fedora.pool.ntp.org` (section 1). Turned off, a time and date field appears and nothing else changes; setting the clock by hand is Settings' (FR19).
- **On Next,** first run calls `SetTimezone(zone, interactive=false)` and `SetNTP`.
- **12 or 24 hours** is not asked: the bar follows the region until the person sets `clock-format` (LN8).

**FR12. Privacy and updates** (decision 4: inform only). One screen that says what the machine does by itself and offers the one choice first run can honour.

- **Location services,** a switch, off by default. On, applications may ask for the machine's position through the location portal, and the position is estimated by sending the names and signal of nearby Wi-Fi networks to BeaconDB, geoclue's default service (section 1). The choice is stored in the hand-off and becomes the user's `org.gnome.system.location enabled` at the first login (FR15). Whether the portal honours that key without GNOME Shell is spike F4; until the spike passes, the switch is not shown and the key keeps its default, `false`.
- **What this computer contacts by itself,** a list read from `/usr/share/athanor/first-run/contacts.toml`, a file the image ships so that the screen stays true when a service changes. Release 1 lists: system updates (the registry of `doc_update_trust.md`, every 6 hours), application updates and the application catalogue (the Flathub system remote, `doc_software.md` decision 6), network time (`2.fedora.pool.ntp.org`), the connectivity check (`fedoraproject.org`, every 5 minutes, to detect sign-in pages). Each entry names its purpose and where it is turned off. A change to any of these contacts updates the file in the same change; a test compares the file with the units and configuration the image ships (FR20).
- **What it never sends:** no usage data, no crash reports and no identifier of the person. Athanor ships no telemetry and no crash reporter (`abrt` is not installed, section 1); the sentence states a fact of the image and is checked by the same test.
- **Updates.** The screen says that system updates download in the background, never restart the computer by themselves, wait on metered connections, and are applied when the person chooses "Restart to update" (SH11, UT11, UT12); and that applications update by themselves. There is no switch here (decision 4); the settings live in Settings. *Amended on 2026-10-06 (A2-5, #150):* the screen says this of feature updates, and says that security updates apply at the next shutdown or restart the person starts, with a notice, and can be undone in one step (SH11, `doc_update_trust.md` UT13).
- **Remote login** is off on a new install (`doc_software.md` decision 2) and is not mentioned: it is turned on in Software's features.
- **Wi-Fi addresses.** One line says that the machine shows each Wi-Fi network a different, stable hardware address (section 1, `stable-ssid`).

**FR13. Appearance** (decision 5: included). Light or dark, and the accent.

- **Two large previews,** light and dark, and the row of the nine accent presets of VL3, with purple preselected. The previews redraw with the chosen accent. The wallpaper-derived accent is not offered here: it needs the person's wallpaper, which does not exist yet.
- **The screen itself follows the choice** at once, through the libadwaita style manager of the first-run process only.
- **Stored in the hand-off** as `color-scheme` and `accent`, values of `org.athanor.desktop.appearance` (VL4), and applied at the first login through the one function of `athanor-style` (FR15).
- **High contrast and large text** are not on this screen: they are in the accessibility menu from the first frame (FR16).

**FR14. Your account.**

- **Fields:** full name; user name; password; password again. The account is an administrator and the screen says so in one line: "This account can change system settings and install software." There is no choice: the first account must be able to administer the machine, and `root` stays locked.
- **The user name is derived** from the full name while the person has not edited it: lower case, letters of the Latin alphabet with accents removed (Unicode decomposition, then the base letter), other characters dropped, spaces and hyphens joined, at most 32 characters, and a leading letter. A full name that yields nothing, such as one in a non-Latin script, leaves the field empty with the hint "Use letters a to z". The field checks the rule of FR6 as the person types and says which character is not allowed.
- **Password quality** uses libpwquality with the system's configuration, the same check the helper repeats (FR6). The screen shows libpwquality's own message when the check fails, translated by its own catalog, and a strength bar from its score when it passes. Next is enabled only when the check passes and both fields match.
- **The password is handled** as at the greeter: a `GtkPasswordEntry`, no clipboard copy, the text moved into a `Zeroizing` buffer when Next is pressed, the entries cleared at once.
- **No automatic login, no password hint, no picture.** After first run the person logs in at the greeter with the new password, so that PAM opens the login keyring with it (`/etc/pam.d/greetd`, `pam_gnome_keyring`, `doc_lock_and_prompts.md` section 1) and the password is proven once before the person depends on it. The picture is set later in Settings.

**FR15. Finishing, and the hand-off to the first session.**

- **On "Start using Athanor"** in the Ready screen, in this order:
  1. the client writes `/var/lib/athanor-first-run/handoff.toml` atomically (write to a temporary file in the same directory, `fsync`, `rename`), mode 0644;
  2. it sends the account request to the helper (FR6);
  3. on `OK`, it exits 0 and the greeter appears (FR3); on `ERR`, it stays on the account screen with the reason and the password fields emptied.
- **The hand-off holds no secret:**

  ```toml
  schema = 1
  user = "anna"                # the user name sent to the helper
  region = ""                  # empty: the region follows the language (LN7)
  location = false             # FR12
  color-scheme = "light"       # FR13, a value of org.athanor.desktop.appearance
  accent = "purple"
  accessibility = ["screen-reader", "large-text"]   # the AX13 feature names that are on
  ```

  It is written before the account exists, so that a crash between the two leaves either no account (first run runs again and rewrites it) or an account with its hand-off. A file with an unknown `schema`, an unknown key or an invalid value is ignored whole, with an entry in the journal.

- **Region.** First run does not offer a region separate from the language; `region` stays empty and the region follows the language (LN7). Settings changes it.
- **At the first login,** `athanor-session` runs `athanor-first-run apply-handoff` before it reads the user's languages and region (LN6, LN7), so the first session already has them. The subcommand, headless and bounded to 2 s:
  - does nothing when `$XDG_STATE_HOME/athanor/first-run-applied` exists, or when the file's `user` is not the logged-in user;
  - writes `org.gnome.system.locale region` when non-empty, `org.gnome.system.location enabled`, and the appearance through `athanor-style`'s apply function (VL4);
  - turns on, never off, the accessibility keys of AX2 for each named feature, as `athanor-a11y apply-greeter` does (AX13);
  - writes the marker, so it never runs again for that user.
    A failure is logged and the session starts with its defaults; the hand-off never blocks a login.
- **The greeter learns the accessibility choices too.** At its start, when its own stored choices have never been written, the greeter reads the hand-off's `accessibility` list and stores it as its own, so a person who set up the machine with the screen reader hears it at the greeter that follows and at every later boot (AX13, persistence). This is an amendment to AX13 (section 3).
- **The file stays** in `/var/lib/athanor-first-run`, owned by `athanor-firstrun`, which no process uses any more. It contains nothing that is not already visible in the account's settings.

**FR16. Accessibility from the first frame.**

- **Parity with the greeter.** The session of `athanor-firstrun` runs `athanor-a11y.service`, `athanor-a11y-gate.service` and `orca.service` in its own user manager, outside the client's sandbox, with the confinement of AX3 and AX4, exactly as the greeter's user does (AX13; spike A1 of `doc_accessibility.md` and F1 here). Super+Alt+S starts and stops the screen reader from the first frame, and Super+Alt+A opens the accessibility menu.
- **The menu** is the greeter's: screen reader, magnifier, high contrast, large text, on-screen keyboard (the embedded widget of AX12), colour inversion and filter. It writes the AX2 keys of `athanor-firstrun`'s own settings through the filtered session bus, as the greeter writes `greetd`'s.
- **Every screen** follows AX6 and AX7: its title is announced when it appears, the focus lands on its first control (the language list's search field, the full-name field), the order of Tab follows the reading order, and errors are announced as alerts. Every control is reachable with the keyboard alone and the pointer alone.
- **A spoken hint** (decision 6): when no key and no pointer event has arrived for 15 s on the first screen and the screen reader is off, first run says once, through speech-dispatcher in the system language, "To use a screen reader, press Super, Alt and S", and shows the same sentence on screen. It does not repeat.
- **Large text and high contrast** apply to the first-run screens at once, through libadwaita, and travel to the first session through the hand-off (FR15).

**FR17. Look and layout.**

- **Full screen** on every output; the pages are drawn on the output that holds the pointer at start, and the others show the hearth wallpaper of the factory accent (VL8). A page is at most 600 logical pixels wide, centred.
- **The smallest screen** first run supports is 1024 × 600 logical pixels in every language of the test matrix, and 1.0, 1.25, 1.5 and 2.0 scale; nothing is cut off and nothing scrolls sideways.
- **Type and geometry** are the visual language's: `title-1` for each screen's title, libadwaita's own sizes elsewhere (VL6, VL7). The mark appears on the first screen and the Ready screen, never as decoration elsewhere (VL8).
- **Motion** follows `enable-animations` of `athanor-firstrun`'s settings: page transitions of 300 ms, none when reduced motion is on (VL9).

**FR18. Failure and recovery.**

- **A crash of the client** ends cosmic-comp and the unit, which restarts in 1 s (FR3). First run starts at the Language screen with every applied value preselected: the system language and keyboard, the time zone, the network already joined. Nothing typed on the account screen is kept.
- **A crash loop.** After five failures in a row within 5 minutes (`StartLimitBurst=5`, `StartLimitIntervalSec=300`), the unit stays failed and greetd starts. The greeter then has no account to greet; it shows "This computer has no account yet. Restart to try again." with the restart button, instead of the card for the nonexistent `athanor` (section 1). That message is a change to the greeter (section 3).
- **Power lost** before the account exists: the next boot runs first run again. Power lost after `CreateUser` and before `SetPassword` returns: the account exists without a password, so the machine is no longer pending and the greeter cannot log in to it. The helper prevents this window as far as it can by rolling back on any error it sees (FR6); for the case of power lost inside that window, `pending` also counts as pending a machine whose only regular account was created by the helper and has no usable password, read from AccountsService's `PasswordMode` and `Locked` properties; this extension is verified in spike F5 together with the time AccountsService needs to report a new account.
- **No network** never stops first run (FR10, FR11).
- **A refused call** (localed, timedated or NetworkManager answers with an error) is shown on its screen with the service's message and a Try again button; first run does not move on with a value it could not apply.

**FR19. Exclusions of the first release,** each recorded with its reason:

| Excluded                                                                | Reason                                                                                                                                       |
| ----------------------------------------------------------------------- | -------------------------------------------------------------------------------------------------------------------------------------------- |
| Setup screens for accounts created after the first (existing-user mode) | Settings already holds every choice; a second program asking the same questions at login doubles the surface (decision 8)                    |
| A welcome tour of the desktop                                           | it describes surfaces that are still changing stage by stage (`doc_shell.md`, SH1); reconsider when the shell's stages are complete          |
| Online accounts, cloud sign-in, mesh identity                           | no online-accounts service is designed; the mesh's authentication in the fleet crates is not real (issue #57), so first run generates no key |
| Directory and enterprise login (realmd, SSSD)                           | no specification covers it; it needs its own threat model                                                                                    |
| Parental controls                                                       | no parental-controls service in the image                                                                                                    |
| Fingerprint enrolment                                                   | opt-in, after login, in Settings (`doc_lock_and_prompts.md` D8)                                                                              |
| Account picture                                                         | set in Settings; it needs the camera portal or a file chooser                                                                                |
| Device name                                                             | stays `athanor`; a person's name in the host name is broadcast to every network (FR5)                                                        |
| Hidden, enterprise and captive-portal networks                          | joined later from the session (FR10)                                                                                                         |
| Region separate from the language, input methods, clock format          | in Settings (LN15); the defaults follow the language                                                                                         |
| Translated city names in the time-zone list                             | the tz database's names are English; translating them needs a city database with translations (libgweather or CLDR) not yet chosen           |
| Setting the clock by hand                                               | only with network time off, in Settings                                                                                                      |
| Third-party repository choice                                           | the image's remotes are decided by `doc_software.md` decision 6                                                                              |
| Telemetry, crash reporting and their switches                           | Athanor has neither; the prototype's switches did nothing                                                                                    |
| Disk encryption and TPM enrolment                                       | the installer's; `athanor-tpm-luks-seal.service` is disabled until 1.1 and 1.0 unlocks with the passphrase (A2-27, #131, #145); first run changes no disk                                                           |
| systemd-homed accounts                                                  | decision 2                                                                                                                                   |

**FR20. Tests.**

- **Without a display, on every change in CI:**
  - `pending` against fixture NSS sources (an empty set, a uid of 999, of 1000, of 60000, of 60001, a homed record), with `UID_MIN` and `UID_MAX` read from a fixture `login.defs`;
  - `names`: the derivation of user names from full names in Latin, accented, Greek, Cyrillic, Arabic, Han and empty input; the validation of FR6, including every reserved name;
  - `handoff`: round trip, atomic write, rejection of an unknown schema, key or value;
  - `zones`: preselection for a single-zone country, a multi-zone country, `UTC` and an unknown country;
  - `layouts`: the Latin check for `us`, `it`, `fr`, `de`, `ru`, `gr`, `ara`, `il`, `th`, `jp`;
  - `steps`: which screens apply for each combination of connectivity and Wi-Fi presence.
- **The helper,** in CI, against an in-process `zbus` server that implements the AccountsService methods FR6 calls: a peer outside the cgroup is refused; a request while an account exists is refused; each malformed field is refused with its name; a failure of `SetLanguages` or `SetPassword` deletes the account; the hash it sends starts with `$y$` and verifies with `crypt_rn` against the password; the request buffer cannot grow beyond 4096 bytes.
- **The polkit rule,** in the dev VM: `pkcheck` for each of the four actions as a process of `athanor-firstrun` in an active local session answers yes; as the same user in an inactive or remote session, and as any other user, the defaults apply; any other action falls through.
- **The contacts file:** a test reads `contacts.toml` and fails when the image's NetworkManager connectivity URI, chrony pool, update timer interval or Flatpak remotes are not listed, or when a listed one no longer exists.
- **Surface cases:** each of the nine screens in SH13's matrix for our own surfaces (scale 1.0 and 1.5, light and dark, English, German and the right-to-left pseudo-locale: 12 cases), 108 cases, plus the two high-contrast variants at scale 1.0 in English (18 cases), with fixtures for NetworkManager (no device, wired, Wi-Fi with networks) and timedated.
- **Accessibility** (ST7): with Orca in the test rig, each screen's title and first control are announced; the whole sequence is completed with the keyboard alone.
- **The ISO acceptance** (`forge/test/iso`) gains a second case whose kickstart declares no user: after the install, `athanor-first-run.service` is active and `greetd.service` is waiting; the existing case, which declares `collaudo`, shows the unit skipped and the greeter running.

**FR21. Construction.** Each step merges on its own. The unit is installed but not enabled by preset until step 5, so the shipped image keeps Anaconda's account screen until then. From step 2 on, each build is installed on the reference laptop and in the dev VM from a kickstart that declares no user, and the maintainer judges it on screen before it merges.

1. **Foundations, with no visible change.** The crate with its GTK-free modules and their tests; `pending`; the sysusers file; `athanor-oobe` removed from the workspace (decision 9).
2. **The session the maintainer sees:** spike F1 first; the unit, the PAM service, the session script and the confined client with its filtered buses; the Language and Keyboard screens with FR5's rule (decision 7); the accessibility menu and the AT units in the first-run user's manager (FR16).
3. **The account:** the maintainer approves the helper's implementation plan (FR6), then the helper and its socket, the account screen and the Ready screen, the greeter's "no account" message (FR18). From here a disk image built from `disk.toml` can be set up by its first user.
4. **The rest of the screens and the hand-off:** Network, Time zone, Privacy and updates (with spike F4 for location), Appearance; the hand-off and `apply-handoff`, the change to `athanor-session`, the greeter's seeding of its accessibility choices; the spoken hint (decision 6).
5. **The switch:** spike F2, then `iso.toml` disables the Users and Timezone modules (decision 1), the preset enables `athanor-first-run.service`, the ISO acceptance gains its second case, and the surface cases and the accessibility checks run in CI. Register entry F-settings-33 becomes `have`.

## 3. Changes to other documents

Applied with the approval of this document.

- **Requests from the languages, accessibility and visual-language specifications, met:**
  - `doc_languages.md` LN16 and section 3: first run uses exactly the interface of LN16 and the lists of `athanor_i18n::offered_languages()` and `athanor_compositor_client::keyboard::available()` (FR8, FR9); first run's own subject is the system user `athanor-firstrun` (FR2); the polkit rule that lets it call `SetLocale` and `SetX11Keyboard` is `30-athanor-first-run.rules`, which sorts before `40-athanor-locale1.rules` and names that subject (FR5), approved as decision 7; the time zone is set through timedated (FR11). LN16 item 2 (the first user's `Languages` and region) is met by the helper's `SetLanguages` (FR6) and by the hand-off applied before `athanor-session` reads the region (FR15).
  - `doc_languages.md` LN7, **owed amendment** (ruling, 2026-10-05): the login sequence gains the first-run hand-off step, `athanor-session` running `athanor-first-run apply-handoff` before it reads `Languages` and `org.gnome.system.locale region` (FR15).
  - `doc_accessibility.md` AX13, **owed amendment** (ruling, 2026-10-05): the `athanor-firstrun` user is a third pre-login place, after the greeter and the lock screen, where the AT units run, with the same menu and shortcuts (FR16); and the greeter's persisted choices are seeded once from the hand-off when the greeter has never stored any (FR15), an addition to "Persistence at the greeter".
  - `doc_visual_language.md` VL4 and VL8: `apply-handoff` is one more caller of `athanor-style`'s apply function; the mark appears in first run (FR17).
- **`doc_software.md`:** decision 2 (remote login off on new installs) and decision 6 (Flathub, automatic application updates) are stated in FR12 and not repeated as choices. No change.
- **`doc_platform_experience.md`, section 2, "Il Primo Avvio (OOBE)" (`:24-33`):** superseded by this document. First run is not part of the greeter, generates no mesh key and signs in to no service (FR19).
- **`shell-features.md:380`, F-settings-33:** the note becomes "specified in `doc_first_run.md`"; the status moves with FR21 step 5.
- **`system/disk_config/iso.toml`:** `org.fedoraproject.Anaconda.Modules.Users` and `org.fedoraproject.Anaconda.Modules.Timezone` move from `enable` to `disable` (decision 1, FR21 step 5).
- **`system/disk_config/disk.toml:5-9`:** the comment says that a disk image without a user is set up by first run.
- **`system/athanor-install.ks`:** the comment at `:40-42` stops saying that systemd-homed encrypts the user's home (decision 2), in FR21 step 5; `sshd` leaves it through `doc_software.md`, not here.
- **`forge/specs/athanor-system-config`:** `athanor-session` gains the call of FR15; `greetd.service.d/20-athanor-first-run.conf` orders greetd after first run (FR3).
- **`forge/specs/athanor-greeter-ui`:** with no account to greet, the greeter shows the message of FR18 instead of a card for `athanor`; it seeds its accessibility choices from the hand-off (FR15).
- **The Settings specification (`doc_settings.md`):** owns every later change of what first run set, and the pages that FR19 sends there (device name, region, clock, input methods, account picture, fingerprint, additional accounts). It writes the same stores (FR5, FR12, FR13); nothing in the hand-off is a store of its own.
- **The portal specification (`doc_portal.md`):** the location setting of the xdg-desktop-portal frontend's own implementation, which has no backend of Athanor's, is `org.gnome.system.location enabled` if the frontend reads that key (FR12, spike F4 decides); the portal specification owns no mechanism for it.

## 4. Open doubts

1. **The decisions of section 6** were taken on 2026-10-05; none is open. The helper's implementation plan still needs the maintainer's approval before code (FR6).
2. **The spikes,** each a short probe on the image as shipped, run in the construction step that needs it. A spike that fails sends its rule back to the maintainer.
   - **F1 is the gate.** Nothing from FR21 step 2 on merges before F1 passes. If F1 fails, these rules fall and return to the maintainer: FR3 (first run as a system unit with a PAM session on VT 1, before greetd); FR5's polkit rule, which needs the subject local and active; FR16, the AT units in `athanor-firstrun`'s user manager, and with it the spoken hint of decision 6; and FR4's filtered session bus, which needs that user's session bus. FR6's admission by cgroup depends only on the unit's name and moves with whatever replaces FR3. The rules that stand regardless are FR1's pending condition, FR6's checks and protocol, the screens' logic (FR8 to FR14), the hand-off (FR15) and the exclusions (FR19).
   - F1. The kiosk unit (FR3): cosmic-comp started by `athanor-first-run.service` with `PAMName=`, `TTYPath=/dev/tty1`: whether logind gives the session seat0, the active state and a user manager with a session bus for `athanor-firstrun`; which session class to set; whether polkit sees the subject as local and active; whether PipeWire and Orca run there (with spike A1 of `doc_accessibility.md`); and whether greetd takes VT 1 cleanly after the unit ends.
   - F2. Anaconda with the Users and Timezone modules disabled in bootc-image-builder's `anaconda-iso` (FR1, decision 1): no account or root screen, the installation completes, `root` is locked on the installed system, and which `/etc/localtime` the installation leaves. In the same run: whether AccountsService's `CreateUser` gives the new account ranges in `/etc/subuid` and `/etc/subgid`, which rootless containers in Software's developer mode need (`doc_software.md` decision 6); if it does not, the helper adds them with `usermod --add-subuids` and `--add-subgids` after `SetPassword`, and that step joins FR6. *Amended on 2026-10-06 (A2-22, #150):* F2 runs on Anaconda's web interface, the installer of release 1.0.
   - F3. The keyboard at the disk passphrase prompt (FR9): after `SetX11Keyboard` with `convert` true on a bootc installation with an encrypted disk, which layout the initrd's prompt uses at the next boot.
   - F4. Location (FR12): whether xdg-desktop-portal 1.20.4 refuses location requests while `org.gnome.system.location enabled` is false without GNOME Shell (its binary carries the message "Location services disabled", `strings`), and whether geoclue serves the portal with no agent of ours.
   - F5. The pending check after a power loss (FR18): AccountsService's `PasswordMode` and `Locked` for an account created without a password, how long it takes to report a new account, and whether reading it from the `pending` helper, which runs as `athanor-firstrun` in `ExecCondition=`, is reliable at boot.
   - F6. libpwquality and libxcrypt from Rust (FR6, FR14): whether maintained crates exist or a small FFI module is written, and whether the password can be passed without a copy that escapes zeroization.
3. **The size of the client's memory** is not budgeted: first run is not resident. The first measurement is recorded, not gated.
4. **`subject.local` for a system user** in a session started by a unit is assumed true on seat0; F1 checks it, because the rule of FR5 depends on it.
5. **The greeter's choice of account** reads `/etc/passwd` and requires a shell ending in `bash`, `zsh` or `fish` (section 1). AccountsService's `useradd` uses the default shell of `/etc/default/useradd`; that it is `/bin/bash` on the image is unverified. F1's run checks it.
6. **The spoken hint** depends on speech-dispatcher running for `athanor-firstrun` (decision 6); F1 covers it with Orca.
7. **The manual kickstart.** `system/athanor-install.ks` (the manual ISO build of `system/README.md:128`) declares no user, so with it first run creates the account exactly as with `iso.toml`. Its comment that systemd-homed encrypts the user's home is unverified and contradicts decision 2; section 3 corrects the comment. Whether the file stays next to `iso.toml` is outside this document.
8. **The hand-off's `user` key** ties it to one account name. If the person deletes that account and creates one of the same name, the new account receives the old choices at its first login; the consequence is cosmetic and no fix is designed.

## 5. Acceptance

In the dev VM installed from a kickstart that declares no user, and on the reference laptop installed from the ISO; the first eight in CI where marked, the rest judged by the maintainer on screen.

1. On first boot `athanor-first-run.service` is active and greetd is not; on a machine installed with a declared user the unit is skipped, not failed, and the greeter starts (CI, ISO acceptance).
2. The first frame shows the Language screen with the mark; Super+Alt+S starts Orca within 1 s and the screen's title and search field are announced; 15 s of inactivity produce the spoken hint once (decision 6).
3. Choosing Italiano switches every string on screen, libadwaita's included, before the Keyboard screen appears; `/etc/locale.conf` reads `LANG=it_IT.UTF-8`.
4. Choosing a Russian layout adds `us` as a second layout, and the greeter afterwards shows the layout switcher.
5. With no network, the Network screen appears on the laptop and "Set up later" completes first run; joining a WPA2 network creates a connection that is active after the next boot with nobody logged in.
6. A process running as `athanor-firstrun` outside the unit's cgroup (started with `systemd-run --uid`) is refused by the account helper; a second request after the account exists is refused (CI for the logic, VM for the unit).
7. `pkcheck` for `org.freedesktop.accounts.user-administration` as `athanor-firstrun` in the first-run session does not answer yes; the four actions of FR5 do (VM).
8. After "Start using Athanor", exactly one account exists with uid 1000, in `wheel`, with a `$y$` hash in `/etc/shadow`; the greeter greets it, the password logs in, and the login keyring is unlocked.
9. The first session is in the chosen language; with the screen reader and large text turned on during first run, both are on in the greeter that follows, after a restart of the machine, and in the first session; the appearance and accent are the chosen ones.
10. Killing the client on the Time zone screen brings first run back within 2 s at the Language screen, with the language, keyboard and time zone already chosen preselected.
11. Every screen fits 1024 × 600 logical pixels in German and in the right-to-left pseudo-locale, and the whole sequence is completed with the keyboard alone (CI surface cases, then on the laptop).
12. The Privacy screen lists exactly the contacts the image makes; the contacts test fails when a contact is added to the image without the file (CI).
13. No file under `/etc` is changed by first run except through localed, timedated, NetworkManager and AccountsService: a comparison of `/etc` before and after, in the VM, shows only `locale.conf`, `vconsole.conf`, `X11/xorg.conf.d/00-keyboard.conf`, `localtime`, the connection file, `passwd`, `shadow`, `group`, `gshadow`, `subuid`, `subgid` and AccountsService's own files.
14. Added on 2026-10-06 (A2-27), rewritten for A2-35 (#131, #145): installed in the dev VM with Secure Boot off and the enrolment declined in MokManager at the first boot, first run shows the Secure Boot key screen; "Prepare again" shows a one-time password different from the installer's, and `mokutil --list-new` as root then lists the project certificate. Installed with Secure Boot on and the enrolment confirmed, `mokutil --test-key` on the shipped certificate reports it enrolled and the screen is not shown. A process outside the unit's cgroup is refused by the enrolment helper (VM).

## 6. Decisions taken

Taken by the maintainer on 2026-10-05, each as recommended in revision 0.

1. **The first account is created in first run** (FR1, FR21 step 5). Anaconda's Users and Timezone modules are disabled in `iso.toml`; the installer keeps its own language and keyboard, the disk and encryption. Reason: every installation path (ISO, disk image, `bootc install`, a machine handed over to a new owner) ends at the same screens, with accessibility from the first frame, and a preinstalled or refurbished machine reaches its next owner without the previous one's account. Kickstarts that declare a user skip first run, so tests and the dev VM are unchanged.
2. **A classic account through AccountsService** (FR6), not systemd-homed. Reason: the greeter, rootless containers and every tool already handle classic accounts, and the home is protected by the disk encryption chosen in the installer; homed needs its own specification with the greeter, the lock and containers.
3. **A dedicated root helper creates the account** (FR5, FR6): one account, only while none exists, only from the first-run unit's cgroup, with its input checked again. Reason: a standing grant of `org.freedesktop.accounts.user-administration` would let its holder create any number of administrators from a client that parses Wi-Fi names an attacker in radio range controls, and `polkit.spawn` cannot express the check without blocking every authorisation and breaking on the order of calls. The helper's implementation plan needs the maintainer's approval before code (FR6).
4. **Updates: inform only** (FR12), no switch. Reason: the update model never acts without the person's confirmation (SH11), and a switch on the first screen would only add a way to leave a new machine without security updates. *Amended on 2026-10-06 (A2-5, #150):* the update model applies security updates at the next shutdown or restart the person starts, with a notice and a one-step way back, and feature updates after the person confirms; the reason for having no switch stands.
5. **The appearance screen is included** (FR13): light or dark and the nine accents, applied at the first login through `athanor-style`. Reason: one screen on a hand-off that already exists; dark mode is a comfort need for people sensitive to light.
6. **The screen-reader hint is spoken once and shown** (FR16) after 15 s without input on the first screen. Reason: a blind person cannot find a button they cannot see, and one sentence once costs everyone else little.
7. **`30-athanor-first-run.rules` is approved** (FR5): four actions (`locale1.set-locale`, `locale1.set-keyboard`, `timedate1.set-timezone`, `timedate1.set-ntp`), the user `athanor-firstrun`, local and active only. Reason: the four actions are settings the person is choosing at that moment, localed and timedated validate their own input, and the rule cannot apply after first run because nothing runs as that user any more.
8. **No setup screens for later accounts in release 1** (FR19). Reason: Settings holds every choice, and a second program asking the same questions at login doubles the surface; reconsider when Settings exists and the screens can be shared.
9. **`system/athanor-oobe` is retired** (FR1, FR21 step 1). Reason: it writes `/etc` directly, stores switches no program reads, and stays outside the visual language and the confinement model; nothing of it carries over. Done: ADR-0073 deleted it from the tree.
