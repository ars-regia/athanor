# Athanor session lifecycle

Status: **revision 1, draft, awaiting review (2026-10-06).** It is the specification that maintainer decision A2-20 (#156) asks for: one document owns the chain that starts and stops the user's desktop session, its environment, the order of its units, and what happens when one of them crashes. Before it, eight specifications amended that chain piecemeal and gave it two different `PartOf=` rules and seven crash policies (audit 2 of 2026-10-05, desktop report, findings 2, 3, 7, 13 and 15). From this revision the other specifications cite it instead of editing the session files themselves. It also owns the crash notices of maintainer decision A2-24 (#156) and the format of the "Needs" lines from which `scripts/verify.py docs` builds the cross-specification dependency graph.

## 1. Context

- **What binds this document.**
  - `doc_shell.md`: SH1 (the replacement rule), SH3 (of COSMIC only cosmic-comp stays), SH8 (the crash-loop protection of five failures in ten minutes on `CLOCK_BOOTTIME`, `doc_shell.md:130`).
  - `doc_shell_standard.md` ST5: recovery within 1 s of a `SIGKILL`, and the start of bar and dock within 1.5 s of the compositor's start.
  - `doc_lock_and_prompts.md` LP2, LP10, LP11 and LP12: the lock and the polkit agent are user units, the lock is never given up, the agent also serves `org.gnome.keyring.SystemPrompter`.
  - `doc_session_daemons.md` SD4, SD5, SD7, SD8, SD10, SD17, SD18 and SD22: the idle daemon, its inhibitors, the broker, the application units, autostart, the daemons' units, their failures, and the retirement order of COSMIC's components.
  - `doc_languages.md` LN6 and LN7 (`LANG`, `LANGUAGE`, the `LC_*` variables) and `doc_accessibility.md` AX3, AX5 and AX9 (the screen reader, its gate, `XCURSOR_*`).
  - Maintainer decision A2-19 (#161): no telemetry; a "Report a problem" command prepares an issue the user reviews and submits.
- **How a session starts today** (branch `shell-specs` at `da940ab8`, read 2026-10-06).
  1. greetd runs `athanor-session` as the user (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-session:2-4`).
  2. It stops whatever a previous session left in the user manager: `systemctl --user stop athanor-session.target` and `reset-failed` (`athanor-session:11-14`).
  3. It exports `XDG_CURRENT_DESKTOP=Athanor:COSMIC`, `XDG_SESSION_TYPE=wayland` and `XDG_DATA_DIRS` with `/usr/share/athanor/cosmic-defaults` first (`athanor-session:18,19,25`), and runs `athanor-usbguard-hook unlock` (`:29`).
  4. It execs `cosmic-comp` with `/usr/bin/athanor-desktop` as the compositor's client, under `systemd-cat -t athanor-session` (`:33`).
  5. `athanor-desktop` reads the session class from logind (`forge/specs/athanor-system-config/SOURCES/usr/bin/athanor-desktop:29-33`) and imports `WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_SESSION_CLASS` into the user manager (`:34`). It supervises `cosmic-greeter` (the locker) and `cosmic-idle` as its own children with a back-off that doubles up to 60 s and never gives up (`:36-102`), and execs `systemctl --user start --wait athanor-session.target` (`:104`).
  6. `athanor-session.target` has `BindsTo=graphical-session.target`, wants and follows `graphical-session-pre.target`, wants `xdg-desktop-autostart.target` and is ordered before it, requires and follows the readiness gate `athanor-desktop.service`, and wants `athanor-skel-sync`, `cosmic-bg`, `cosmic-settings-daemon` and `cosmic-osd` (`forge/specs/athanor-system-services/SOURCES/usr/lib/systemd/user/athanor-session.target:7-24`). Our shell is enabled under it by user presets (`WantedBy=athanor-session.target`, for example `forge/specs/athanor-bar/athanor-bar-1.0.0/data/athanor-bar.service:77`); `system/Containerfile:189` asserts the wants links of bar, dock and shelld.
  7. The readiness gate `athanor-desktop.service` is a oneshot with `RemainAfterExit=yes` that waits up to 10 s for a `wayland-info` round trip, with a 15 s start timeout (`athanor-desktop.service:17-23`). It carries no `PartOf=`.
- **How it stops today.** The logout is `systemctl --user stop athanor-session.target` (`athanor-desktop:14`; the bar's power menu calls `StopUnit` on it, `forge/specs/athanor-bar/athanor-bar-1.0.0/src/ui/logind.rs:17,86-92`). The wait returns, `athanor-desktop` exits, cosmic-comp exits with its client, greetd shows the greeter. `graphical-session.target` has `StopWhenUnneeded=yes` and the target binds to it, so it stops once `athanor-session.target` is gone (`/usr/lib/systemd/user/graphical-session.target` of systemd 258.11 on the image, read 2026-10-06), and every unit with `PartOf=graphical-session.target` stops with it.
- **The units as shipped.**

| Unit | Ordering and binding | Restart | Source |
|---|---|---|---|
| `athanor-shelld.service` | `After=` and `PartOf=graphical-session.target` | `on-failure`, 100 ms, start limit 10 in 600 s, SH8 counter through `ExecStopPost` | `forge/specs/athanor-shelld/athanor-shelld-1.0.0/data/athanor-shelld.service:5-24` |
| `athanor-bar.service` | wants and follows shelld; `After=` and `Requisite=athanor-desktop.service`; `PartOf=graphical-session.target` | as shelld | `athanor-bar.service:5-26` |
| `athanor-dock.service` | `After=` and `Requisite=athanor-desktop.service`; `PartOf=graphical-session.target` | as shelld | `forge/specs/athanor-dock/athanor-dock-1.0.0/data/athanor-dock.service:4-24` |
| `athanor-launcher.service` (built, not enabled) | as the dock | `on-failure`, 1 s, `RestartSteps=5`, `RestartMaxDelaySec=60s`, start limit 10 in 600 s | `forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/athanor-launcher.service:6-25` |
| `cosmic-bg`, `cosmic-osd`, `cosmic-settings-daemon` | as the dock; `WantedBy=graphical-session.target` | `on-failure`, 2 s | `forge/specs/athanor-system-services/SOURCES/usr/lib/systemd/user/cosmic-bg.service:4-20,38` and its siblings |
| `athanor-update-notify.service` | `After=` and `PartOf=graphical-session.target`; `WantedBy=graphical-session.target` | `on-failure`, 30 s | `forge/specs/athanor-update/SOURCES/usr/lib/systemd/user/athanor-update-notify.service:4-19` |
| `athanor-skel-sync.service` | oneshot, before bar and dock; `WantedBy=niri-session.target graphical-session.target` | none | `forge/specs/athanor-system-services/SOURCES/usr/lib/systemd/user/athanor-skel-sync.service:4,12,31` |
| `app-athanor-<id>@<random>.service` | no ordering and no binding: only `Type=exec`, `ExitType=cgroup`, `CollectMode=inactive-or-failed` | none | `system/athanor-compositor-client/src/unit.rs:86-99` |
| `xdg-desktop-portal.service` (upstream) | `PartOf=graphical-session.target`, `Type=dbus` | none | `/usr/lib/systemd/user/xdg-desktop-portal.service` on the image, read 2026-10-06 |

- **The SH8 counter.** `system/athanor-unit/src/crash_loop.rs:1-9,16-17` counts failures in the unit's runtime directory; the bar, after five, keeps running on the vendor layout without favourites (`forge/specs/athanor-bar/athanor-bar-1.0.0/src/main.rs:55-66`).
- **Core dumps are off** image-wide: `Storage=none`, `ProcessSizeMax=0` (`forge/specs/athanor-base-config/SOURCES/etc/systemd/coredump.conf.d/99-disable.conf`). With `panic = "abort"` a crashing program leaves a journal line and nothing else.
- **What follows from the facts.**
  - Two `PartOf=` rules are written: `graphical-session.target` in every shipped unit and in SD17 and PT3, `athanor-session.target` in LP2 (`doc_lock_and_prompts.md:55`).
  - The readiness gate has no `PartOf=`, so after a logout it stays active (exited) for as long as the user manager lives. A new login inside that time, or with lingering enabled, finds the gate already active: it does not run again, and the units ordered after it no longer wait for the new compositor.
  - Application units are bound to nothing: at logout they are not stopped by the session, and nothing orders them against the portal frontend, the polkit agent or the lock.
  - Nothing removes the imported variables from the user manager at logout, so a unit started between two sessions inherits the old `WAYLAND_DISPLAY`.
  - `LANG` and `XCURSOR_*` are not in the import list, though LN6 and AX9 need them in the user units.

## 2. Decisions

**SN1. What this document owns.** The environment of the session and its import list (SN2), the `PartOf=` and `WantedBy=` rule (SN3), the start order (SN4), the stop order (SN5), the crash classes (SN6), the guards' degraded modes and the idle daemon's persisted inhibitors (SN7), crash notices and local reports (SN8), the end state of `athanor-desktop` (SN9) and of `XDG_CURRENT_DESKTOP` (SN10), and the "Needs" graph (SN11). Another specification that adds a session unit states its class (SN6) and any `After=` edge it needs by citing this document; it does not edit `athanor-session`, `athanor-desktop`, `athanor-session.target` or `athanor-desktop.service` itself. A change to any of these is a revision of this document.

**SN2. The environment.**

- **Exported by `athanor-session`,** before it execs cosmic-comp, in this order:
  1. `XDG_CURRENT_DESKTOP` (SN10) and `XDG_SESSION_TYPE=wayland`, as today;
  2. `XDG_DATA_DIRS` with Athanor's defaults first, as today, because cosmic-comp is a child of this script and not of the user manager; the user manager gets the same value from `/usr/lib/environment.d/60-athanor-cosmic-defaults.conf` (`forge/specs/athanor-calmo/SOURCES/usr/lib/environment.d/60-athanor-cosmic-defaults.conf`);
  3. after `athanor-first-run apply-handoff` (LN7), `LANG` and `LANGUAGE` from AccountsService (LN6) and the `LC_*` variables of the region (LN7);
  4. `XCURSOR_SIZE` and `XCURSOR_THEME` from the GNOME keys (AX9).
- **The import list.** `athanor-desktop` imports into the user manager exactly: `WAYLAND_DISPLAY DISPLAY XDG_CURRENT_DESKTOP XDG_SESSION_TYPE XDG_SESSION_CLASS LANG LANGUAGE LC_NUMERIC LC_TIME LC_MONETARY LC_PAPER LC_ADDRESS LC_TELEPHONE LC_MEASUREMENT XCURSOR_SIZE XCURSOR_THEME`. A variable the session did not set is not imported, so an unset `LANGUAGE` leaves the manager's value alone.
- **Never imported:** `XDG_DATA_DIRS` (environment.d covers the manager), `XDG_SESSION_ID` (units find their session through logind, LP2), `XDG_ACTIVATION_TOKEN` (one per launch, BR2), `DBUS_SESSION_BUS_ADDRESS` (the manager's own).
- **Removed at the end.** The readiness gate gains `ExecStop=` that runs `systemctl --user unset-environment` over the same list, so a unit started after the logout and before the next login never inherits a dead display. The list lives in one file read by both `athanor-desktop` and the gate, so the two never diverge.
  - Needs: LN6, LN7, AX9.

**SN3. One binding rule.**

- **`PartOf=graphical-session.target`** for every user unit Athanor ships for the session, the readiness gate included, and for every application unit the broker creates. This is the convention of systemd.special(7) ("Such services should have `PartOf=graphical-session.target`") and of the upstream units the session runs, such as `xdg-desktop-portal.service`, so one rule covers ours and theirs and one stop transaction stops them all. `athanor-session.target` keeps `BindsTo=graphical-session.target`, so the session stops when either target does.
- **`WantedBy=athanor-session.target`** in `[Install]`, enabled by a user preset, for every unit the session starts eagerly. A D-Bus-activated unit (`athanor-osd`, the portal backend) has no `[Install]` section.
- **Every unit that connects to the compositor** has `After=` and `Requisite=athanor-desktop.service`, as the bar, the dock and the launcher do today. A headless unit (shelld, sessiond, broker) has neither, so D-Bus activation works before the gate.
- **Rejected: `PartOf=athanor-session.target`.** It stops our units in a transaction of their own, before `graphical-session.target` becomes unneeded and stops the upstream ones, so no `After=` edge between an application of ours and the portal frontend would order their stops. The only unit that writes it, LP2's, is not built yet.

**SN4. The start order.** Each edge is an `After=`; a `Wants=` is named where the edge must also pull the unit in.

| Edge | Why | From |
|---|---|---|
| graphical units after `athanor-desktop.service` | no client connects before the compositor serves a surface (`athanor-desktop.service:5-13`) | today |
| `athanor-bar` after `athanor-shelld` (and wants it) | the bar reads notifications from shelld (BR1) | today (`athanor-bar.service:5-6`) |
| shell surfaces (bar, dock, launcher, `athanor-osd`, Settings, overview, control center) after `athanor-lock` and `athanor-polkit-agent` | a surface that asks for authorisation finds the agent and its `SystemPrompter` registered (LP11, LP12); at the stop, the guards outlive the surfaces (SN5) | each guard's enabling step (LP18 steps 3 and 5) |
| `orca.service` after `athanor-a11y-gate.service`, and wants it, in Orca's Athanor drop-in | the reader reaches the accessibility bus only through the gate (AX5) | the gate's step (AX18 step 6) |
| application units after `xdg-desktop-portal.service`, `athanor-polkit-agent.service` and `athanor-lock.service` | the portal frontend is up before an application asks it, and outlives it at the stop; the same for the agent and the lock | the broker (SD22 step 6) |
| autostart after `athanor-session.target` is active | today through `Before=xdg-desktop-autostart.target` (`athanor-session.target:10-11`); from the broker on, SD10 launches the entries once the target is active, and the broker is wanted by the target, so it is up first | today, then SD10 |

- **The guards are ready when they serve.** `athanor-lock`, `athanor-polkit-agent`, `athanor-idle`, `athanor-broker` and `athanor-a11y-gate` are `Type=notify` or `Type=dbus` and report ready only once they lock, are registered with polkit, hold their timers, own their name or listen, so an `After=` edge to them means what it says.
- **The start threshold of ST5 holds** with these edges: the bar now waits for two more units. If it does not hold on the reference machine, the edge is kept and the guard's start is made faster; ST5 is not relaxed (ST2).
  - Needs: LP2, LP11, AX5, SD8.

**SN5. The stop order.** systemd stops in the reverse of the start order, so SN4 gives the order of a logout:

1. application units, while the portal frontend, the agent and the lock still run;
2. the shell surfaces;
3. the headless services: `athanor-shelld` after the bar (it follows shelld at the start), sessiond, the broker, the wallpaper;
4. the guards: the lock and the polkit agent last of ours, so a session that ends while locked never shows its content;
5. the readiness gate, which unsets the imported environment (SN2).

The stop of each unit is bounded by the manager's default stop timeout. An application that does not exit within it is killed; this is the declared cost of ending a session.

**SN6. Crash classes.** Every session unit belongs to one class, and its unit file says which in a comment that cites this rule.

| Class | Members | Policy | Never |
|---|---|---|---|
| **Guards** | `athanor-lock`, `athanor-idle`, `athanor-polkit-agent` (with the `SystemPrompter`), `athanor-broker`; and, by this document, `athanor-sessiond` and `athanor-a11y-gate` | `Restart=always`, `RestartSec=100ms`, `RestartSteps=5`, `RestartMaxDelaySec=30s`, `StartLimitIntervalSec=0`; a degraded mode while down (SN7) | given up; a guard restarts for as long as the session lasts |
| **Surfaces and their services** | `athanor-bar`, `athanor-dock`, `athanor-shelld`, `athanor-launcher`, `athanor-osd`, Settings, the overview, the control center, `athanor-wallpaper`, `athanor-update-notify`, `athanor-a11y` (the watcher) | `Restart=on-failure`, `RestartSec=100ms`, `StartLimitBurst=10` over `StartLimitIntervalSec=600`; the program's own SH8 counter decides its degraded state after five failures in ten minutes on `CLOCK_BOOTTIME` | a second, different counter |
| **Activated helpers** | the portal backend `xdg-desktop-portal-athanor` | no `Restart=`: the next D-Bus call activates it again; systemd's default start limit cuts a tight loop and expires, so a later call succeeds | a give-up that lasts the session |
| **Session steps** | `athanor-desktop.service`, `athanor-skel-sync.service` | oneshot, no restart; the gate's failure fails the target and so the login, which greetd reports | a restart |

- **Why sessiond and the gate are guards.** sessiond reports every other crash (SN8) and holds the USBGuard notice; a reporter that gives up hides every later failure. The gate fails closed (AX5): while it is down no screen reader reaches the bus, which for a blind user is a session that cannot be used. Both are additions to the four guards of A2-20 (#156), for the maintainer to confirm (section 4, question 8).
- **The lock** keeps LP10's 100 ms first restart; the back-off only lengthens a loop. A single `SIGKILL` after a healthy run must restart within 100 ms (ST5), which depends on systemd returning to the first step (section 4, question 2).
- **COSMIC's units and children** keep their policies until they leave (SD22, OD15): `RestartSec=2s` in the three units, and `athanor-desktop`'s supervisor, which already never gives up, for cosmic-greeter and cosmic-idle.

**SN7. Degraded modes of the guards, and the idle daemon's inhibitors.**

| Guard | While it is down |
|---|---|
| lock | cosmic-comp keeps the defunct lock: every output stays dark and locked, and the new instance takes the lock over (LP10) |
| idle | no automatic blank, lock or suspend; the lock before sleep is the lock's own and still works (SD18); the inhibitors come back with the restart (below) |
| polkit agent | polkit finds no agent for the session (fallback off, LP11); what the caller then receives is polkitd's (section 4, question 3); the session never falls back to another agent |
| broker | launches fail closed with the caller's toast (SD7); running applications are unaffected |
| sessiond | a blocked USB device stays blocked; inserted drives queue in udisks; no crash notice is shown until it is back, and it then reports what the manager recorded meanwhile (SN8) |
| a11y gate | readers cannot connect; Orca is restarted after the gate (SN4) |

- **No guard locks the session because it failed.** A crash loop with a 30 s ceiling would lock the user out every 30 s. The notice of SN8 tells the user instead.
- **The idle daemon keeps its inhibitors across a crash** (A2-20, #156), as `athanor-shelld` keeps its unread notifications (ST5):
  - At every change it writes its table to `$XDG_RUNTIME_DIR/athanor-idle/inhibitors.json`, mode 0600, through a temporary file and a rename: for each inhibitor its cookie, the unique bus name of its owner, its application id, its kind (idle or suspend only) and its reason. The directory is the unit's `RuntimeDirectory=` with `RuntimeDirectoryPreserve=yes`, so it survives a restart and leaves with `$XDG_RUNTIME_DIR` at the end of the login.
  - At start it restores each entry whose owner still has its unique name on the bus (`NameHasOwner`); unique names are never reused on a bus, so a present name is the same client. Other entries are dropped, each with one journal line at info. New cookies continue above the highest restored one, so an `UnInhibit` after the restart finds its inhibitor.
  - A file that does not parse is removed, the event logged at warning, and the daemon starts with no inhibitor.
  - Needs: SD4, SD5.

**SN8. Crash notices and local reports** (A2-24, #156). Core dumps stay off; `athanor-sessiond` tells the user and keeps a report the user can attach to "Report a problem" (A2-19, #161).

- **The source is the user manager,** not files the units write: sessiond subscribes to the manager and reads, for each unit of the table in SN6, the service properties `Result`, `NRestarts`, `ExecMainCode`, `ExecMainStatus` and `InvocationID`. A failure is a run whose `Result` is not `success`. Rejected: reading the SH8 records of each unit's runtime directory, which only the programs with a counter write.
- **The notice,** through `org.freedesktop.Notifications`, at most one per unit per session: at the first failure of a guard; at the fifth failure in ten minutes of a surface or service (SH8's point); when an activated helper reaches its start limit. It names the component in the user's words ("The screen lock stopped unexpectedly and was restarted") and offers "Report a problem".
- **The report.** For every failure sessiond writes `$XDG_STATE_HOME/athanor/crash-reports/<UTC time>-<unit>.txt`, mode 0600, through a temporary file and a rename: the unit, the image version from `/usr/lib/os-release`, the time, `Result`, the exit code or signal, `NRestarts`, and the last 50 journal lines of the failed invocation (`_SYSTEMD_INVOCATION_ID`). It keeps the 20 most recent reports and deletes older ones. Nothing is sent anywhere: the "Report a problem" flow shows the report to the user, who decides whether to attach it.
- **sessiond's confinement** (SD17) gains one write directory, `$XDG_STATE_HOME/athanor/crash-reports`, and read access to the user's journal.
  - Needs: SD1.

**SN9. The supervisor after COSMIC's children leave.** `athanor-desktop` keeps three duties: the session class, the import (SN2) and the exec of `systemctl --user start --wait athanor-session.target`. Its supervisor loop (`athanor-desktop:36-102`) leaves with its last child: cosmic-greeter at LP18 step 3, cosmic-idle at SD22 step 1. From then the lock and the idle daemon are guards of SN6 and nothing in the session is supervised outside the user manager.

- Needs: LP2, SD4.

**SN10. `XDG_CURRENT_DESKTOP`.** `Athanor:COSMIC` until SD22 step 8 retires cosmic-settings-daemon, then `Athanor`. The suffix is kept until then because COSMIC components and desktop entries with `OnlyShowIn=COSMIC` read it. The portal's configuration does not depend on it once `athanor-portals.conf` ships (#157), since xdg-desktop-portal reads the file of the first desktop name that has one. The change is made in the same step as the retirement, after the check of section 4, question 5.

- Needs: SD22.

**SN11. The "Needs" lines and the dependency graph.**

- **The format.** A dependency of one requirement on another is a line of its own, inside the requirement, that begins with `Needs:` (after an optional list marker), followed by requirement identifiers separated by commas and a final full stop: `Needs: LP2, SD4.` An identifier is one to three capitals and a number, defined in exactly one specification of `docs/architecture/` as the bold head of a requirement (`**LP2.`). The line belongs to the nearest requirement head above it. It means: the requirement above cannot be built before the named ones are.
- **What `scripts/verify.py docs` checks:** that every `Needs:` line is inside a requirement and parses; that every identifier it names is defined exactly once; that no requirement needs itself; and that the graph has no cycle, naming one when it does. It reports the number of requirements and edges as a note.
- **Prose dependencies** written before this revision ("Needs the lock of ...", SD22) are not lines of this format and are not read. Each specification converts them at its next revision; until then the graph holds only what this document declares.

**SN12. Tests.**

- A `verify.py` check over the shipped user units: the binding rule of SN3, the `WantedBy=` rule, the edges of SN4 that are in force, and the class policy of SN6 per unit. It runs in CI without a display.
- The "Needs" check of SN11, with its unit tests in `scripts/tests/`.
- In the idle daemon's crate, without a display: the inhibitor table's write and restore against fixtures (owner present, owner gone, unparseable file, cookie continuation).
- In sessiond's crate: the failure detection from fixtures of the manager's properties, the notice rule (one per unit, the thresholds per class), the report's content and its rotation.

**SN13. Construction.** Each step merges on its own.

1. **The rule on what ships:** `PartOf=graphical-session.target` and `ExecStop=` on `athanor-desktop.service`; the import list in one file; `WantedBy=athanor-session.target` for `athanor-update-notify`; `niri-session.target` out of `athanor-skel-sync`'s `[Install]`; the launcher on its class's restart values; the check of SN12.
2. **The language and cursor variables** in `athanor-session` and the import list, with LN6, LN7 and AX9.
3. **The guards,** each in its own enabling step with its class and edges: the lock (LP18 step 3), the agent (LP18 step 5), the idle daemon with SN7's inhibitors (SD22 step 1), sessiond with SN8 (SD22 step 3), the broker with the application units' binding and edges (SD22 step 6), the gate (AX18 step 6).
4. **The supervisor's end** (SN9), once steps 3 of LP18 and 1 of SD22 have both merged.
5. **`XDG_CURRENT_DESKTOP=Athanor`** at SD22 step 8 (SN10).

## 3. Changes to other documents

Applied in this revision, each marked as amended on 2026-10-06 by maintainer decision A2-20 (#156):

- `doc_session_daemons.md`: SD8 (the application unit's binding and edges come from SN3 and SN4), SD17 (the units' binding, ordering and restart come from SN3, SN4 and SN6; sessiond's write directory of SN8) and SD18 (guards are never given up; the idle daemon restores its inhibitors, SN7).
- `doc_lock_and_prompts.md`: LP2 (`PartOf=graphical-session.target` instead of `athanor-session.target`) and LP10 (the lock's restart is the guard class of SN6, with its first restart at 100 ms).
- `doc_shell_standard.md` ST5: unit restart settings and the session's start order belong to this document.

To be applied by each specification at its next revision, citing this document:

- `doc_shell.md` SH8 and `doc_bar.md` BR1: the crash-loop protection is the class "surfaces and their services" of SN6.
- `doc_osd.md` OD13: the OSD is in the same class; its start limit is SN6's.
- `doc_portal.md` PT3: the backend is an activated helper, without `Restart=on-failure`.
- `doc_settings.md` SE5: `RestartSec=100ms`, the class value.
- `doc_accessibility.md` AX3 and AX4: the gate is a guard; Orca's drop-in gains the edge of SN4.
- `doc_languages.md` LN6: the import list is SN2's.
- Every specification with prose dependencies, starting with SD22: its "Needs" become lines of SN11.

## 4. Open questions

Each is answered on the acceptance VM or the reference laptop before the step that depends on it.

1. Whether `systemctl --user start --wait` (`athanor-desktop:104`) returns when cosmic-comp dies without a logout, and so whether the target stops; today the next login's `athanor-session:13` clears what is left.
2. Whether systemd 258 returns a unit with `RestartSteps=` to its first delay after a healthy run, or only after a manual start. If only after a manual start, a guard that once crash-looped restarts with the long delay for the rest of the session, and ST5's 1 s fails for it; the guard class then needs another mechanism.
3. What polkitd answers a caller when the session has no registered agent.
4. Which unit serves the Secret Service once oo7-daemon replaces gnome-keyring (A2-7, #151), and whether application units need an `After=` edge to it for a save at logout.
5. Whether cosmic-comp, or any package that stays after SD22 step 8, reads `COSMIC` in `XDG_CURRENT_DESKTOP`.
6. Whether `athanor-sessiond`, under SD17's confinement, can read the user's own journal on the image (journald's file permissions); without it the report carries no journal lines.
7. Whether user units receive `LANG` today from the system locale through the user manager, which decides whether a user without a language of their own needs the import at all.
8. Whether the maintainer confirms `athanor-sessiond` and `athanor-a11y-gate` as guards beyond A2-20's four.
9. Whether the bar's start, ordered after the lock and the agent, still meets ST5's 1.5 s on the reference machine.

## 5. Acceptance

The specification stands when, on the acceptance VM:

1. after login, every unit Athanor ships for the session reports `PartOf=graphical-session.target` (`systemctl --user show -p PartOf`), and the check of SN12 passes on the image;
2. after a logout, while the user manager still runs, no unit of ours is active, `athanor-desktop.service` is inactive, and `systemctl --user show-environment` holds none of SN2's list;
3. the journal of a logout with one application running shows the application stopped before the bar and the bar before the lock and the agent (SN5);
4. ten `SIGKILL`s in a row of each guard leave it active, never `failed`, with no delay above 30 s, and the eleventh `SIGKILL` after a healthy minute is followed by a restart within 1 s;
5. an `org.freedesktop.ScreenSaver` inhibitor held by a test client survives a `SIGKILL` of `athanor-idle`, and the client's `UnInhibit` afterwards succeeds;
6. a bar killed with `SIGSEGV` produces one notice and one report under `$XDG_STATE_HOME/athanor/crash-reports/`, mode 0600, and no core dump (`coredumpctl list` is empty);
7. `python3 scripts/verify.py docs` builds the graph of SN11 without a dependency problem.
