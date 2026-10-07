# Desktop and session specifications (SPD)

Snapshot: 91aefb9f. Scope covered:

- Read in full: doc_shell, doc_shell_standard, doc_bar, doc_launcher, doc_compositor, doc_session, doc_visual_language.
- Read in full by delegated readers and spot-checked at the source: doc_settings, doc_software, doc_first_run, doc_portal, doc_session_daemons, shell-features.md, and the desktop entries of components.toml.
- Read selectively, for decision conflicts, stale claims, dead links and units: doc_lock_and_prompts, doc_osd, doc_accessibility, doc_languages, doc_files.
- Decision records 0001-0072.
- Cross-checked against the shell code under `forge/specs/athanor-{bar,dock,shelld,launcher,xdg-desktop-portal-athanor,system-config,system-services,desktop-ui}`, `system/athanor-{compositor-client,portal,style}`, `Cargo.toml`, `forge/config/packages.json` and `system/Containerfile`.

Not covered:

- The line-by-line requirement text of doc_lock_and_prompts, doc_osd, doc_accessibility, doc_languages and doc_files; their delegated reader stopped on a rate limit.
- doc_overview, doc_disks and doc_recovery, except where the specs in scope cite them.
- Runtime behaviour, since nothing was built or booted.

Paths are relative to the snapshot root. `docs/architecture/` is abbreviated `A/`.

## Summary

The desktop specifications are detailed and mostly carefully reasoned. But they have stopped describing the system, for three reasons:

1. **Accepted decisions were not applied.** The second audit's decisions (A2, records 0036-0072) were accepted on 2026-10-05/06, but most specs never received them.
   - The appearance store (0057), the trusted path (0041), Nix for every user (0053), Alt+Tab (0061) and the removals of 0045 are each contradicted by the specs that should carry them.
   - In several cases the merged code already follows the decision while the spec says the opposite.
2. **"Today" statements are wrong at the snapshot.** About thirty such statements, line anchors and links are false. Thirteen documents cite `doc_control_center.md`, which is not in the tree.
3. **Nearly every spec is unreviewed.** Every spec in scope except doc_software is a draft or "awaiting approval". Built and merged code rests on them, and they change through appended amendments that contradict the text they amend.

Three product defects sit behind the documents:

- The register marks the volume keys `have`, although they are broken on the shipped image.
- No document can say when the desktop locks on idle.
- No specification owns the microphone, camera and location indicators, and screen sharing is routed to `none`.

The fix is mostly mechanical:

- Apply each accepted record in place.
- Resolve every `doc_*.md` link and `path:line` in `verify.py docs`.
- Generate the register from data.
- Adopt one spec template with fixed headings.

## Findings

### SPD-01 Five specs contradict decision 0057 on the appearance store; the merged portal already follows 0057

- Severity: high
- Category: contradiction
- Where: docs/decisions/0057-session-coherence.md:18, :22; A/doc_visual_language.md:50-69 (VL4), :68; A/doc_shell.md:101 (SH5); A/doc_settings.md:10, :155, :192; A/doc_first_run.md:229, :253; A/doc_portal.md:12, :89, :101, :262, :283; A/doc_session_daemons.md (SD6); system/athanor-portal/src/settings.rs:4-8
- Evidence:
  - 0057 decides: "GNOME keys as the single appearance store (Athanor keys only for accent mode and computed accent; portal Settings backend; athanor-portals.conf; reverses VL4 mirrors and AX2)". Its Consequences list five documents as "Applied by", each "(on shell-specs)".
  - VL4 still defines `org.athanor.desktop.appearance` with color-scheme, contrast, accent, accent-mode and accent-computed, and says the GNOME keys "are mirrors".
  - doc_settings:155: Appearance "never writes the GNOME mirrors … (VL4)".
  - doc_portal:89: `org.freedesktop.appearance` comes "from `org.athanor.desktop.appearance` (VL4)".
  - SD6 reads the accent and scheme from the Athanor schema.
  - The merged backend (`system/athanor-portal/src/settings.rs:4-8`) already does what 0057 says: the colour scheme is in `org.gnome.desktop.interface color-scheme`, the contrast in `org.gnome.desktop.a11y.interface high-contrast`, and `org.athanor.desktop.appearance` "holds only `accent-mode` and `accent-computed`".
- Standard: ADR (Nygard/MADR), where a later accepted record overrides the spec; one source of truth per fact; ISO/IEC/IEEE 29148 (consistent).
- Recommendation:
  - Rewrite VL4 around the GNOME keys, keeping the Athanor schema for accent-mode and accent-computed only. Amend SH5, SE13/SE18, FR13/FR15, PT5/decision 1 and SD6 in place.
  - Correct 0057's "Applied by" list so it names the documents that actually carry the decision.
  - Add a `verify.py docs` check: every document named under an accepted record's "Applied by" must cite that record.
- Needs a decision: no

### SPD-02 The trusted path is specified as "our own patch, never proposed upstream", reversed by decision 0041

- Severity: high
- Category: contradiction
- Where: docs/decisions/0041-trusted-path-on-cosmic-comp.md:18; A/doc_shell.md:57; A/doc_compositor.md:54, :70 (patch 5); A/doc_lock_and_prompts.md:178, :249, :315 (D10)
- Evidence:
  - 0041 (accepted 2026-10-05): "D10 trusted path built on cosmic-comp PR #1441; guarantee stated against confined apps only; agent identified by /usr exe + unit; reveal key proposed upstream (reverses 'never proposed')".
  - doc_shell:57: the trusted path is "our own cosmic-comp patch … with no upstream proposal".
  - doc_compositor patch 5: upstream status "None, by the maintainer's decision (D10)", exit "None while the compositor offers no auth…".
  - doc_lock_and_prompts:315: "D10 … our own cosmic-comp patch only, with no upstream proposal".
  - None of the three names PR #1441, the confined-apps-only scope of the guarantee, or how the agent is identified.
- Standard: ADR precedence; ISO 29148 (consistent, complete). For a security mechanism, the guarantee's scope must be stated where implementers read it.
- Recommendation:
  - Rewrite LP13/D10, CO3 patch 5 and SH2 from 0041: the base is PR #1441, the guarantee is stated against confined applications only, the agent is identified by its `/usr` executable and its unit, and the reveal key is proposed upstream.
  - Give patch 5 a real exit condition: upstream merges #1441 or an equivalent.
- Needs a decision: no

### SPD-03 doc_software contradicts decision 0053 (Nix for every user), which claims doc_software applies it

- Severity: high
- Category: contradiction
- Where: docs/decisions/0053-nix-for-every-user.md:18, :22; A/doc_software.md:137-144 (SW9, SW10), :185, :220, :278 (decision 3a), :368 (acceptance 12)
- Evidence:
  - 0053: "Nix for every user … declarative per-user tool list auto-rebuilt when the image bumps the nixpkgs pin … Software offers CLI tools only from a curated allowlist … no GUI apps from Nix … automatic GC". It says it is "Applied by … doc_software.md".
  - doc_software keeps "Developer mode: Nix tools" (SW9). It installs by name from an index of the whole of nixpkgs (:185, "on the order of 100,000 packages"), and decision 3a keeps Nix "only in developer mode".
- Standard: ADR precedence; ISO 29148 (traceable). A false "Applied by" misleads agents that trust the record.
- Recommendation:
  - Rewrite SW9, SW10, SWd and acceptance 12 for 0053:
    - Nix outside developer mode;
    - the allowlist as a shipped file, with its path and schema named;
    - the declarative list rebuilt on a pin bump;
    - generations shown in the updates UI, naming which document owns that UI;
    - automatic GC;
    - `trusted-users = root`.
  - Drop spike S2, the full-nixpkgs catalog.
- Needs a decision: no

### SPD-04 The image still ships what the desktop decisions removed, and does not ship what they chose

- Severity: high
- Category: contradiction
- Where:
  - docs/decisions/0045-cleanup-of-dead-components.md; docs/decisions/0052-firefox-as-flatpak.md
  - A/doc_software.md (rev 3, decision 7); A/doc_shell.md SH3 (:82)
  - forge/config/packages.json:112-113, :116, :132-135, :146-150, :187
  - forge/specs/athanor-desktop-ui/athanor-desktop-ui.spec:13-20
  - forge/specs/athanor-bar/athanor-bar-1.0.0/data/favorites.toml
- Evidence:
  - 0045 approves the removal of the COSMIC apps, Thunar, foot, swaybg, swaylock, virt-manager and qemu. packages.json still lists all of them:
    - `upstream_desktop`: cosmic-term, cosmic-files, cosmic-edit, cosmic-store, Thunar, thunar-archive-plugin, thunar-volman, swaybg, swaylock;
    - `upstream_core`: qemu-img, qemu-kvm, virt-manager.
  - The GNOME default set of doc_software rev 3 is absent from packages.json and the Containerfile. The one exception is `nautilus`, pulled in by a `Requires` of athanor-desktop-ui.
  - athanor-desktop-ui also `Requires: … firefox` (an RPM), against 0052 (Firefox as a Flatpak), which packages.json:187 already lists.
  - The bar's factory favourites are `com.system76.CosmicFiles`, `CosmicTerm`, `CosmicEdit` and `CosmicSettings`, the very apps the decisions remove.
  - swaylock is a second, unmanaged screen locker beside the lock specification's single idle policy.
- Standard: ADR lifecycle (an accepted decision is applied, or tracked as a pending consequence); one source of truth.
- Recommendation:
  - Make one change that applies 0045, 0052 and doc_software decision 7 together: drop the listed packages, add the GNOME set, drop the `firefox` RPM `Requires`, and point the favourites at the GNOME desktop ids.
  - Add a `verify.py shipped` check that every package named as removed by an accepted record is absent from packages.json.
  - Until that change lands, give each record a "Consequences: pending" line with the issue number.
- Needs a decision: no

### SPD-05 The register marks the volume keys and headset dialog `have` while the shipped image breaks them, and the session-daemon template repeats the cause

- Severity: high
- Category: contradiction
- Where:
  - A/shell-features.md:286, :287, :294 (F-osd-01, -02, -09); A/doc_osd.md:23
  - forge/specs/athanor-system-services/SOURCES/usr/lib/systemd/user/cosmic-settings-daemon.service:21-22
  - A/doc_session_daemons.md:223-235 (SD17, SD18, SD20)
- Evidence:
  - doc_osd:23 records "a defect on the shipped image": cosmic-settings-daemon panics because it cannot bind its varlink socket ("Read-only file system"). Its unit sets `ProtectSystem=strict` and `ProtectHome=read-only` with no `ReadWritePaths=` or `RuntimeDirectory=`. The fix is deferred to the retirement step (M5).
  - The register keeps all three rows `have`, with the defect only in the note.
  - SD17 prescribes the same `ProtectSystem=strict` and `ProtectHome=read-only` template for the four planned daemons, with no `RuntimeDirectory=`, `StateDirectory=` or `ReadWritePaths=`. Yet:
    - athanor-idle writes `$XDG_RUNTIME_DIR/athanor-idle/inhibitors.json` (:235), while its Landlock line says it "writes nothing";
    - sessiond writes crash reports under `$XDG_STATE_HOME` (:228).
  - SD20's budgets are never turned into `MemoryHigh=` or `MemoryMax=`, although every shipped shell unit has them (for example, athanor-shelld.service: `MemoryHigh=48M`, `MemoryMax=192M`, `RuntimeDirectory=`, `StateDirectory=`).
- Standard: systemd hardening (an explicit write surface and resource limits per unit); doc_shell_standard ST3 (`have` means it works in the current image); ISO 29148 (verifiable).
- Recommendation:
  - Set F-osd-01, -02 and -09 to `partial`.
  - Ship the scoped fix now (`ReadWritePaths=%t/<socket dir>` or a `RuntimeDirectory=`) rather than waiting for athanor-osd.
  - Replace SD17's prose with one per-unit table: Type, Restart, RuntimeDirectory/StateDirectory, Landlock write set, MemoryHigh/Max derived from SD20 and record 0068, syscall set, address families. Copy the shipped athanor-shelld.service pattern.
- Needs a decision: no

### SPD-06 The wallpaper daemon's sandbox, as specified, cannot run glycin's bubblewrap

- Severity: high
- Category: quality
- Where: A/doc_session_daemons.md SD6 (cites "Cargo.toml:98"; the glycin line is Cargo.toml:95), SD17 (:223-230); system/athanor-preview/src/render.rs:68-87
- Evidence:
  - SD6 decodes wallpapers with glycin, whose loaders "run sandboxed in bubblewrap".
  - The launcher's preview renderer already shows what glycin needs: render.rs:68-87 adds `@mount @privileged` to the syscall filter, allows AF_NETLINK and keeps user namespaces so that bwrap starts.
  - SD17 forbids each of these: `@system-service`, `AF_UNIX` only, and Landlock, which denies mount.
- Standard: ISO 29148 (feasible requirement).
- Recommendation:
  - Do not load glycin inside the confined daemon. Decode through the existing athanor-preview-render path (a transient unit with render.rs's properties, returning pixels over a pipe), so the daemon itself stays AF_UNIX-only with Landlock read-only.
  - Record this as a blocking spike of doc_session_daemons section 4.
- Needs a decision: no

### SPD-07 No document can say when the desktop blanks, locks or suspends on idle

- Severity: high
- Category: contradiction
- Where: docs/decisions/0015-session-daemons.md:34; A/doc_session_daemons.md:22, SD3-SD4; A/doc_lock_and_prompts.md:78-79 (LP5); A/doc_settings.md:140-142 (SE11); A/shell-features.md (F-lock-16)
- Evidence:
  - 0015: "no CosmicIdle default is shipped today, so the desktop never blanks or suspends".
  - doc_session_daemons:22: a fresh account "gets cosmic-idle's compiled defaults (unverified, spike L7a)".
  - LP5 and SE11 state factory values of 5 min blank, 15 min suspend on battery and never on AC, "shipped as the defaults of the schema `org.athanor.desktop.idle`". That schema belongs to athanor-idle, which does not exist.
  - No `com.system76.CosmicIdle` defaults file exists under forge/specs (searched).
  - Suspected: a fresh install never locks on idle. This needs the running image to confirm.
- Standard: OWASP ASVS V3 (session timeout); ISO 29148 (verifiable); one source of truth per fact.
- Recommendation:
  - Ship a `com.system76.CosmicIdle` v1 defaults file now with LP5's values (`screen_off_time` 300000, `suspend_on_battery_time` 900000, `suspend_on_ac_time` None).
  - Add an image acceptance check that reads the effective values, and close spike L7a.
  - State the factory values once (LP5) and link to them from SD3 and SE11.
  - Order the lock before the blank. SD4 locks 500 ms after the outputs go dark, as GNOME does not.
- Needs a decision: no

### SPD-08 No specification owns the microphone, camera and location indicators or switches, and screen sharing is routed to nothing

- Severity: high
- Category: missing
- Where:
  - A/doc_portal.md:193 (PT15); A/doc_bar.md:29, :71; A/shell-features.md:45 (F-bar-16, F-bar-17)
  - A/doc_settings.md:216 (SE21), :261 (SE24); A/doc_first_run.md:216-224 (FR12); A/doc_software.md (SW3); A/doc_local_ai.md:95 (AI6)
  - forge/specs/athanor-xdg-desktop-portal-athanor/xdg-desktop-portal-athanor-1.0.0/athanor-portals.conf
- Evidence:
  - PT15: the microphone, camera and location indicator "belongs to `doc_bar.md`". doc_bar plans only the screen-capture indicator (:71), and the register records F-bar-16 as "missing | not in doc_bar.md".
  - SE21 drops the microphone row: access is "managed in Software". SW3 has no control to revoke it.
  - Settings has no location master switch (SE24 excludes geoclue), while first run writes `org.gnome.system.location enabled` (FR12).
  - AI6 requires "a microphone indicator whenever any capture stream exists".
  - The shipped `athanor-portals.conf` routes `ScreenCast` and `Screenshot` to `none`, so video calls cannot share a screen. The register has no portal surface, so this is recorded nowhere.
  - GNOME, macOS and Windows 11 all ship these indicators and privacy pages as baseline.
- Standard: ISO 29148 (complete); doc_shell_standard ST3 (union of every reference); WCAG 2.2 is not at issue, but privacy parity is the stated baseline.
- Recommendation:
  - Give the indicators one owner: a new BR requirement for F-bar-16/17, a PipeWire node monitor for microphone and camera plus the geoclue clients, with an acceptance case.
  - Add one Privacy page to Settings, as GNOME and macOS have: location, camera and microphone master switches (`org.gnome.desktop.privacy`), and per-app microphone and camera rows that edit the Flatpak override.
  - Rank a ScreenCast backend next. xdg-desktop-portal-cosmic on cosmic-comp's image-copy-capture exists today and beats writing PT-ScreenCast from scratch.
  - Add `portal`, `display` and `input` surfaces to register version 2.
- Needs a decision: yes. Where per-app microphone control lives: one Privacy page in Settings (recommended) or a control in Software.

### SPD-09 Settings and Software are called network-less and confined, yet each can start arbitrary transient units on the session bus

- Severity: high
- Category: vulnerability
- Where: A/doc_settings.md:65-76 (SE6), :71, :228, :288-289; A/doc_software.md:74, :92, SW10
- Evidence:
  - SE6 states "Settings has no network (SE6)" and grants Landlock writes to `$XDG_CONFIG_HOME/autostart`.
  - SE22 and SE27 then start `fwupdmgr …` and the report command "as a transient user unit". Software states the same mechanism for its jobs (`StartTransientUnit`, :74).
  - Nothing filters either program's session bus. Code running in Settings can therefore call systemd's `StartTransientUnit` with any ExecStart (network, no Landlock), or drop an autostart entry.
  - SW10 lists what escapes Software's confinement. SE6 lists nothing.
- Standard: the zero-trust rule in CLAUDE.md; OWASP ASVS V1 (document trust boundaries truthfully); systemd hardening (a confined client reaching an unconfined manager).
- Recommendation:
  - Put Settings and Software behind xdg-dbus-proxy with a policy that allows `StartTransientUnit` only through two small named helpers, one for fwupdmgr and one for the report command, activated by name.
  - Remove `autostart` from Settings' Landlock writes in favour of the Background portal.
  - Until then, add an SW10-style "what escapes it" table to SE6.
- Needs a decision: yes. Filter the session bus of our own apps (recommended), or declare the exception in SE6.

### SPD-10 The launcher spec says every launch goes through os.athanor.Broker1, which does not exist; the launcher starts units itself

- Severity: medium
- Category: contradiction
- Where: A/doc_launcher.md:113 (LA8), :177-180; forge/specs/athanor-launcher/athanor-launcher-1.0.0/src/ui/actions.rs:58-81; system/athanor-compositor-client/src/unit.rs:48-153
- Evidence:
  - LA8: "Every application the launcher or the library starts … starts through `os.athanor.Broker1` (SD7) … This closes, for the launcher, the limit BR2 declares".
  - The code calls `client()?.launch(&app)`, `launch_with`, `launch_command` and `open_uri` on the compositor client, which builds `app-athanor-<id>@<random>.service` and calls `StartTransientUnit` itself.
  - No `Broker1` exists anywhere in the Rust tree.
  - doc_launcher is declared implemented (PR #91), and its "where they pass" section does not mention the gap.
- Standard: ISO 29148 (verifiable; a requirement the implementation does not meet is recorded as open); one source of truth.
- Recommendation: state in LA8 that launches go through the compositor client until SD7 exists, keep the BR2 limit open for the launcher, and add a migration step to doc_session_daemons' plan.
- Needs a decision: no

### SPD-11 Thirteen documents cite specifications that are not in the tree, and several cite ignored or retired locations

- Severity: high
- Category: stale
- Where:
  - `doc_control_center.md` is cited by A/doc_settings.md:11-12, :328, :351-352, :408; A/doc_portal.md:11, :229; A/doc_session_daemons.md (CC2, CC8); A/shell-features.md (CC12); and doc_accessibility, doc_disks, doc_osd, doc_files, doc_lock_and_prompts, doc_visual_language, doc_shell_standard and doc_first_run.
  - `doc_notification_center.md`: A/doc_settings.md:12, :352; A/doc_portal.md:233.
  - `doc_naming.md`: A/doc_settings.md:27.
  - `doc_core_daemons.md`: A/doc_portal.md:236.
  - `.superpowers/` (gitignored, .gitignore:22-23): A/doc_shell.md:3, :9, :269; A/doc_update_trust.md.
  - `.scratch/final-run.log`: A/doc_launcher.md:185.
  - Retired or other branches: "branch `shell-specs` at `da940ab8`" (A/doc_session.md:14); "branch a2/portal-step1" (A/doc_session.md:52); "(on shell-specs)" in the Consequences of records 0036-0072.
- Evidence:
  - The four specifications are absent from `docs/architecture` at the snapshot. doc_control_center exists only on branch `control-center-spec`.
  - Settings decision 1 rests its residency design on "`doc_control_center.md:38`".
  - doc_shell's spike results and approved layout drawings live in a gitignored directory, so no other clone can read them.
- Standard: agents.md and Diátaxis (reference integrity); ISO 29148 (traceable).
- Recommendation:
  - Merge doc_control_center and doc_notification_center into iso-v0, or inline the facts they are cited for.
  - Fold the naming rules into doc_shell_standard.
  - Commit the spike reports under `docs/shell-bench/` or drop the citations.
  - Add a `verify.py docs` check that resolves every `doc_*.md`, repository path and `path:line` in `docs/architecture` and `docs/decisions`.
- Needs a decision: yes. Merge the control-center and notification-center specs now (recommended), or inline the cited facts.

### SPD-12 About thirty "today" statements and line anchors are false at the snapshot

- Severity: high
- Category: stale
- Where, each with what the tree actually says:
  - A/doc_shell.md:33-34 (section 1, "What the platform gives us today"): "the client verifies no image signature … `insecureAcceptAnything`" and "the preset enables `bootc-fetch-apply.timer`". In fact:
    - forge/specs/athanor-update/SOURCES/usr/share/athanor/containers/templates/policy.json.in:6-8 requires `sigstoreSigned` for the athanor-system repositories;
    - 80-athanor-update.preset disables `bootc-fetch-apply-updates.timer`.
  - A/doc_shell.md:139 (SH8) names the `/usr/bin/athanor-cosmic-panel` fallback; :286 (section 7 item 2) changes the accent "in COSMIC Settings"; :294 (item 10) tests the translator that :132 says is gone.
  - A/doc_session.md:33 shows `athanor-skel-sync.service` `WantedBy=niri-session.target`. The shipped unit (forge/specs/athanor-system-services/SOURCES/usr/lib/systemd/user/athanor-skel-sync.service:31) really is `WantedBy=niri-session.target graphical-session.target`, a niri leftover in a shipped file.
  - A/doc_bar.md:143: `scripts/shell-bench/scenarios.py` "not on iso-v0 yet". It exists, beside bench.py, soak.py and machine.py.
  - A/doc_software.md:57: "ours … implements ScreenCast, FileChooser, Camera, Location and Microphone". athanor.portal declares only `org.freedesktop.impl.portal.Settings`.
  - A/doc_software.md:346-348: doc_files, doc_disks and doc_portal are "to be written". All three exist.
  - A/doc_settings.md:19, :38: athanor-settings-rs "is excluded from the workspace (`Cargo.toml:60`)". It is gone from the tree.
  - A/doc_portal.md:26-27 describes release 1.0.0-3 with `Requires: athanor-shell-rs`. The spec is at release 7 with different Requires.
  - A/doc_portal.md:61 calls `system/athanor-portal` "a new library". It is a workspace member.
  - A/doc_portal.md:45, :48 route Screenshot and Background to `athanor`. The shipped conf routes them to `none`, which is correct, since the backend does not implement them.
  - A/doc_first_run.md:30: `system/athanor-oobe` is "a workspace member". It is in `exclude` (Cargo.toml:59).
  - Off-by-N anchors into packages.json:
    - doc_software:22 (:156 cited, really :135);
    - SE23 (:132-133 cited, really :127-128);
    - shell-features rows (5-7 lines off, e.g. cosmic-osd :132 → :125);
    - doc_session_daemons:13 and SD22.
  - A/shell-features.md F-cc-07 cites network.rs:124 for 802.1X. That line is `fn start`, and network.rs has no 802.1X code.
- Evidence: each item was verified against the file named above at 91aefb9f.
- Standard: one source of truth per fact; agents.md (agents act on these statements); ISO 29148 (correct).
- Recommendation:
  - Re-baseline each "today" section against the snapshot.
  - Replace `file:line` anchors into fast-moving files with symbol or package-name anchors.
  - Let the SPD-11 link check also reject an anchor whose line no longer contains the cited symbol.
  - Fix the skel-sync unit's `WantedBy=` in the same change as doc_session SN13.
- Needs a decision: no

### SPD-13 Code is built on specifications nobody approved, and statuses contradict themselves

- Severity: high
- Category: process
- Where:
  - A/doc_shell.md:3; A/doc_launcher.md (status rev 2); A/doc_compositor.md (status); A/doc_shell_standard.md (ST1 "If doc_compositor.md is approved", ST5 proposal)
  - A/doc_visual_language.md:3; A/doc_session.md:3; A/doc_session_daemons.md:3; A/doc_software.md:3, :402-447
  - The wave-1 and wave-2 drafts: doc_settings, doc_first_run, doc_portal, doc_lock_and_prompts, doc_osd, doc_accessibility, doc_languages, doc_files (each line 3: "draft … text not yet reviewed")
  - A/shell-features.md (49 rows `excluded (proposed)`)
- Evidence:
  - doc_shell rev 5 is "awaiting the maintainer's approval; the stage 2 switch (PR #83), doc_bar … and doc_shell_standard are already built on it".
  - doc_launcher rev 2 awaits approval but is merged (PR #91).
  - doc_compositor awaits approval, yet record 0057 treats its patch register as decided.
  - doc_software's line 3 says "Approved, rev 2" and "rev 3 … awaits the maintainer's review", while section 9.4 says a Bazaar comparison is needed "before this document is approved". For 1.0 it is unspecified whether the store is cosmic-store, an own Software or nothing.
  - Accepted records (for example 0015) rest on drafts that were never reviewed.
  - doc_shell:5, :7: "no run of the gate of section 7 is recorded" for stages that are declared done.
  - ST3 says an entry "is left out only by the maintainer", yet 49 proposed exclusions already count as excluded in the surface gates.
- Standard: ISO/IEC/IEEE 29148 (baselined requirements before design); ADR (records reference reviewed specs).
- Recommendation:
  - Hold one review session that approves or amends doc_shell, doc_launcher, doc_compositor and doc_visual_language as built, decides the 49 exclusions as one batch recorded in a decision record, and fixes the 1.0 store.
  - Give every spec one machine-readable status line (draft | approved rev N, date, decision record), checked by `verify.py docs`.
  - Do not merge implementation against a draft without a record that accepts the risk.
  - Run and record the stage 1 and stage 2 gates.
- Needs a decision: yes. The 1.0 store: Bazaar early as a Flatpak (recommended, since it is libadwaita and ends SW1's plain-gtk4-rs exception), an own Software, or cosmic-store until Fedora 45.

### SPD-14 Specs change by appended amendments that contradict their own text

- Severity: medium
- Category: quality
- Where:
  - A/doc_shell.md:149, :157-158 (SH11), :168 (SH12)
  - A/doc_session_daemons.md:224, :235
  - A/doc_software.md:297-329 (decision 7) against :376-390 (section 9)
  - A/doc_first_run.md:5
  - A/doc_session.md section 3 (amendments owed to SH8, BR1, OD13, PT3, SE5, AX3, AX4, LN6, LN11)
- Evidence:
  - SH11's heading says "never applied unconfirmed" and its first paragraph says `systemd-sysupdate` replaces bootc. Two bullets below reverse both: security updates apply at the next shutdown (0040, 0063), and there is no sysupdate (0043).
  - doc_session_daemons' amendments set `Restart=always` and "never given up" over bullets that still read `Restart=on-failure` and "five failures given up".
  - doc_software's decision table still lists an own Files and Disks; section 9 overrides that.
  - doc_session lists nine amendments to other specs that were never applied.
  - The four wave-2 specs handle amendments in three different ways (top of file, appended section, none).
- Standard: Diátaxis (reference states the current truth; history belongs in git and ADRs); RFC 2119 (conflicting normative statements).
- Recommendation:
  - Apply every amendment in place, delete the superseded sentence, and leave an inline "(amended YYYY-MM-DD, record NNNN)" marker.
  - Apply doc_session section 3 to the nine target documents.
  - Forbid appended amendment sections in the spec template (SPD-22).
- Needs a decision: no

### SPD-15 The same fact is specified in several places with different values

- Severity: medium
- Category: redundancy
- Where:
  - Opening latency: A/doc_shell_standard.md ST5 (100 ms at p95 from the input event); A/doc_launcher.md:162 (under 150 ms from the Show call); A/doc_settings.md:379 (acceptance 1, 100 ms on the first Show, while SE-S1 calls a cold start unverified and decision 1 cites a dead link for "misses 100 ms").
  - Crash policy:
    - A/doc_bar.md:30 (BR1: five failures in ten minutes, then error);
    - A/doc_shell.md:139 (SH8 counter);
    - A/doc_session.md SN6 (`StartLimitBurst=10` over 600 s);
    - shipped units: bar, dock and shelld have `StartLimitBurst=10`/600 s with `RestartSec=100ms`, while the launcher has `RestartSec=1s` with `RestartSteps=5` and `RestartMaxDelaySec=60s`.
  - Memory budgets: ST5, A/doc_bar.md:196, SD20 and the units' `MemoryHigh`/`MemoryMax` (bar 96M/192M against ST5's 64 MB PSS).
  - Alt+Tab owner: A/doc_launcher.md:141 (LA12, "plan 3c"), :187 and A/shell-features.md F-overview-10, against record 0061 ("Alt+Tab owned by doc_overview").
  - Unit target: LP2 `PartOf=athanor-session.target` against SN3 `graphical-session.target` (acknowledged in doc_session, still open).
  - The shield's data source: A/doc_shell.md:162 (SH12, "reads one root-owned, world-readable state file") against A/doc_bar.md:110 (BR6, `os.athanor.Update1.State()` over D-Bus). The code (forge/specs/athanor-bar/athanor-bar-1.0.0/src/shield.rs:8-12) uses D-Bus.
  - The float offset of 6 px in both VL2 and VL7.
- Evidence: the values above, each verified at its source.
- Standard: one source of truth per fact; ISO 29148 (consistent).
- Recommendation:
  - Make doc_shell_standard the single owner of cross-surface numbers (latency, crash policy, budget method) and have the surface specs cite it.
  - Generate the unit limits from one table (SPD-05).
  - Change LA12 to "moved to doc_overview (0061)".
  - Rewrite SH12's contract as BR6's D-Bus interface.
  - Make the Settings first-show acceptance a declared cold-start budget, separate from warm shows.
- Needs a decision: no

### SPD-16 SH5's visual identity was never reconciled with doc_visual_language, and VL8 spends the trust seal on decoration

- Severity: medium
- Category: contradiction
- Where: A/doc_shell.md:99-105 (SH5); A/doc_visual_language.md section 3, VL8; system/athanor-style/calmo/tokens.toml:7-12; forge/config/packages.json (`rsms-inter-fonts`); docs/decisions/0061-desktop-minor-decisions.md
- Evidence:
  - SH5 has an indigo accent (`#2e44c2`/`#8898f7`, hue 231), Inter as the only family, the mark "reserved for the trust shield … nowhere else", two hearth images and cosmic-icon-theme.
  - VL has purple `#9141ac`, Adwaita Sans, the mark in four places (greeter, lock, first run, About), eighteen hearth images and adwaita-icon-theme. VL section 3 lists these as changes owed to SH5, never applied.
  - The tokens still say `hue = 231` and `family = "Inter"`.
  - Design: a seal that appears on four decorative surfaces stops being read as a verification signal. SH5's rule was the stronger design.
- Standard: one source of truth per fact; security UX (a trust indicator must be unique to the state it signals).
- Recommendation:
  - Apply VL's palette, font and icon set to SH5 and tokens.toml in one change.
  - Keep the mark exclusive to the shield, as in SH5, and give the greeter, lock, first run and About a wordmark or the hearth instead.
- Needs a decision: yes. Is the mark exclusive to the trust shield? Options: SH5's rule (recommended) or VL8's four placements.

### SPD-17 Components the specs exclude or retire still ship or still build

- Severity: medium
- Category: stale
- Where:
  - forge/config/packages.json:14, :16, :24, :56
  - forge/specs/athanor-desktop-ui/athanor-desktop-ui.spec:13-20
  - A/doc_shell_standard.md:42 (ST3)
  - A/shell-features.md (F-bar-47, F-cc-56, F-launcher-24, F-notif-44, F-settings-35)
  - Cargo.toml:19, :28, :59
  - docs/decisions/0011-wave2-first-run.md; docs/decisions/0021-old-crates-deleted.md
  - A/components.toml (athanor-desktop-ui, athanor-cliphist, athanor-system-services, athanor-system-config)
- Evidence:
  - ST3's example of a zero-trust exclusion is "a clipboard history any application can read". Yet `cliphist` is a custom package in tier 1 and a `Requires` of athanor-desktop-ui. No current surface uses it.
  - `athanor-matugen` and `athanor-dart-sass` serve only the frozen, unpackaged shell, while VL3 chooses the material-colors crate.
  - athanor-desktop-ui ships only a ddcutil udev rule but `Requires` cliphist, ddcutil, grim, slurp, wl-clipboard, brightnessctl, playerctl, wireplumber, nautilus and firefox. components.toml describes it as "default applications, MIME and terminal lists, favourites".
  - `forge/specs/athanor-niri-ipc` is a workspace member (record 0021 says delete). The Ermete-era `system/athanor-greeter` is a member with no spec. `system/athanor-oobe` remains although record 0011 retires it.
  - `custom_packages` still names `secure-boot`, which has no spec directory (SH12 item 7 says it is gone).
  - components.toml marks athanor-system-services and athanor-system-config "missing", although doc_session specifies their units. It has no entry for the ten planned desktop programs (athanor-broker, -idle, -wallpaper, -sessiond, -osd, -library, -overview, -polkit-agent, -osk, -a11y).
- Standard: ADR lifecycle; one source of truth; arc42 building-block view (the registry omits planned blocks).
- Recommendation:
  - Drop cliphist, matugen and dart-sass from the image, unless a record keeps them.
  - Reduce desktop-ui to what a spec names, and fix its components.toml purpose.
  - Delete niri-ipc, `system/athanor-greeter` and `system/athanor-oobe`, as the records say.
  - Remove `secure-boot` from custom_packages.
  - Add `planned` entries for the ten programs, and make components.toml's `item` a requirement id that `verify.py coverage` resolves.
- Needs a decision: no

### SPD-18 The shell feature register cannot be verified and should be generated from data

- Severity: medium
- Category: quality
- Where: A/shell-features.md (538 lines, 362 rows), :3; scripts/verify.py:623 (REGISTER), `register_count_problems`; docs/decisions/0031-shell-features-register-check.md
- Evidence:
  - The file says "Version 1, frozen on 2026-10-04", yet commits 83e10aa7 and bd82bb42 edited it on 2026-10-05. ST3 says the register changes only by a new version.
  - 36 rows say "not verified" or "unverified".
  - Anchors are off (SPD-12), and F-cc-07 overstates a feature.
  - The rows were researched against COSMIC epoch-1.6.0, while the image ships cosmic-comp 1.8.0.
  - The Sources column is about 72% of the row text (measured by a delegated reader).
  - verify.py checks the Counts only. It never checks the evidence, the owner or the status.
- Standard: ISO 29148 (traceable, verifiable); Diátaxis (research, which is explanation, mixed with status, which is reference).
- Recommendation:
  - Move the rows to data (`docs/architecture/shell-features/*.toml`: id, surface, sources with version, status, evidence as path plus symbol, owner requirement id, decision record for exclusions).
  - Have verify.py resolve the evidence and owners, reject a `have` whose evidence is a planned component, and generate the Markdown table and Counts.
  - Version the register by tag, not by an in-file line.
- Needs a decision: no

### SPD-19 The bar parses untrusted input without fuzzing, and notification behaviour is below the reference desktops

- Severity: medium
- Category: missing
- Where: A/doc_bar.md:162 (BR9), BR4; forge/specs/athanor-shelld/athanor-shelld-1.0.0/src/store.rs:9, src/notifications.rs:28; Cargo.toml exclude `tests/fuzz`
- Evidence:
  - BR9 deliberately leaves out fuzzing. Yet the bar and shelld parse the most untrusted input in the session: notification `image-data`, dbusmenu layouts and tray pixmaps, from any client.
  - The parsers are pure Rust, and a `tests/fuzz` workspace already exists.
  - BR4 caps history at 100 (`CAPACITY: usize = 100`) with a 5 s popup. shelld advertises the `persistence` capability. The spec has no per-application notification settings, no lock-screen notification policy and no priority.
  - GNOME, macOS and Windows 11 all offer per-app rules and a lock-screen policy. The fuller notification work exists only on branch `notification-center` (commits d2468982, 41506cb7 are not in the snapshot).
- Standard: OpenSSF Scorecard (Fuzzing); ISO 29148 (complete).
- Recommendation:
  - Add cargo-fuzz targets for the image-data, dbusmenu and pixmap parsers to `tests/fuzz`, run them in CI, and reverse BR9's exclusion.
  - Merge or re-baseline the notification-center specification, so per-app rules and a lock-screen policy have an owner.
- Needs a decision: no

### SPD-20 The launcher's network contacts are not on the first-run contact list, and the list omits DNS

- Severity: medium
- Category: missing
- Where: A/doc_launcher.md:39, :44; forge/specs/athanor-launcher/athanor-launcher-1.0.0/data/athanor-launcher-rates.timer; A/doc_first_run.md:38, :219 (FR12), :325; forge/specs/athanor-system-tweaks/SOURCES/usr/lib/systemd/resolved.conf.d/50-athanor-dns.conf:35-38; docs/decisions/0046-dns-strict-dot.md, 0056-no-telemetry.md
- Evidence:
  - A daily user timer runs `qalc -e` to fetch currency rates (doc_launcher:44), and the web row hard-codes DuckDuckGo (:39).
  - First run's "what this computer contacts" list (FR12) names updates, Flathub, NTP and the connectivity check, but not the rates source, Quad9 DNS over TLS (0046) or the fwupd metadata refresh (suspected; the timer was not checked).
  - The acceptance test (:325) checks only the listed contacts, so it passes with any of these missing.
- Standard: decision 0056 (honest disclosure of contacts); ISO 29148 (complete, verifiable).
- Recommendation:
  - Derive `contacts.toml` from the shipped units and config (rates timer, resolved.conf.d, update timer, fwupd), and make the FR12 test compare the two.
  - Let the user turn currency rates off in Settings.
- Needs a decision: no

### SPD-21 First run: the unit sketch contradicts its failure rule, and the enrolment helper does not pin what it imports

- Severity: medium
- Category: quality
- Where: A/doc_first_run.md:74-97 (FR3), :288 (FR18), :131-135, :179, :243 (FR15)
- Evidence:
  - FR18 relies on `StartLimitBurst=5` and `StartLimitIntervalSec=300`, but the FR3 sketch (`Restart=on-failure`, `RestartSec=1s`) has no StartLimit keys, so systemd's defaults apply instead.
  - The unit is `Before=greetd.service` and `WantedBy=graphical.target` while it waits for a person. Suspected: this holds the boot "activating" long enough for a boot-success check to judge the first boot failed.
  - The MOK helper is "`mokutil --import`, `--hash-file` (root) | the helper's own checks". Nothing states that it imports only a fixed certificate shipped in `/usr` with a compiled-in hash.
  - The hand-off file is mode 0644 and holds the user's accessibility needs.
- Standard: systemd unit conventions; OWASP ASVS (input validation at a trust boundary); data minimisation.
- Recommendation:
  - Put the StartLimit keys in the sketch, and state how greenboot treats a first run left open for 20 minutes, with an acceptance case.
  - Specify that the helper takes no certificate argument and refuses any file whose SHA-256 differs from the compiled value; the MOK chain is on the maintainer's list.
  - Make the hand-off file 0640, owned by the new user, and delete it after it is applied.
- Needs a decision: no

### SPD-22 The specs have no common structure, so neither readers nor checks can find failure, accessibility or acceptance sections

- Severity: medium
- Category: quality
- Where: A/doc_compositor.md (no acceptance section); A/doc_shell.md (acceptance per stage, section 7); A/doc_settings.md, A/doc_first_run.md, A/doc_portal.md (Context, Decisions, Changes, Open doubts, Acceptance, Decisions taken); A/doc_software.md (spikes, risks, section 9 amendments); A/doc_session.md and A/doc_session_daemons.md (same units in both)
- Evidence:
  - The specs have no headings for non-goals, failure behaviour or accessibility. These live inside numbered requirements with different names (SE24, FR19, PT15; FR18, SW6).
  - doc_portal has no behaviour for a backend crash (open sessions, inhibitors, capture). Its accessibility is one line with no acceptance case, and area selection has no keyboard path.
  - doc_compositor carries a patch register with no acceptance.
  - Open doubts carry no owner and no closing step: SE-S11 is decided but still listed, and PT S1-S7 are all open.
  - Cross-cutting content sits in unrelated specs: offline help in doc_software 9.6, and report-a-problem in doc_settings SE27.
  - doc_session and doc_session_daemons both specify the same units.
- Standard: arc42 (one building block per section); ISO/IEC/IEEE 29148 (requirement attributes); WCAG 2.2 (accessibility as an acceptance criterion); Diátaxis.
- Recommendation:
  - Adopt one template in doc_shell_standard with fixed headings: Status, Scope, Non-goals, Requirements, Interfaces, Failure behaviour, Accessibility, Acceptance, Open doubts with owner and closing step, Decisions. Check the headings in `verify.py docs`.
  - Give doc_compositor an acceptance section (one rebase drill per patch).
  - Move unit properties to doc_session alone.
  - Move offline help to a short doc_help.md, and report-a-problem next to SN8.
- Needs a decision: no

### SPD-23 Decision records are not self-contained and their boilerplate is false

- Severity: medium
- Category: process
- Where: docs/decisions/0001-0015 (Decision sections); 0036-0072 (Consequences); docs/decisions/0010-wave2-settings.md:24 against 0026-settings-firmware-via-fwupd.md; docs/decisions/0060-authselect-without-nullok.md:18 against A/doc_lock_and_prompts.md:279 (open doubt 11)
- Evidence:
  - Records 0001-0015 state decisions as option letters ("1 A GNOME keys", "D8 C …"), which cannot be read without the draft they answered.
  - Many Consequences say "No specification document under docs/architecture cites this record yet" while specs do cite them; for example, 0068 is cited at A/doc_shell_standard.md:93. Others say "Applied by … (on shell-specs)", a retired branch.
  - 0010 item 7 (firmware via os.athanor.Lvfs) is superseded by 0026, but both are `accepted` and neither links to the other.
  - 0060 decides "authselect without-nullok now". doc_lock_and_prompts still lists nullok as open doubt 11, and nothing in the tree applies `without-nullok`.
- Standard: ADR (Nygard/MADR: context, options and outcome are self-contained; status and supersession are explicit).
- Recommendation:
  - Rewrite 0001-0015's Decision sections in full sentences.
  - Regenerate every Consequences section from the actual citations (`rg` for the record id), and mark 0010 as partly superseded by 0026.
  - Close doubt 11 by pointing to 0060, and track its application; the auth module stays with the maintainer.
- Needs a decision: no

### SPD-24 Gaps against GNOME Shell, macOS and Windows 11 with no owner

- Severity: medium
- Category: missing
- Where: A/shell-features.md (surfaces list); A/doc_settings.md SE12, SE15, SE24; A/doc_shell.md SH10 and open doubt 9; A/doc_shell_standard.md ST1
- Evidence:
  - Quick settings exist only in the absent control-center spec (SPD-11). Brightness and the power profile sit in the battery module, which hides on desktops (ST1 notes this).
  - Not specified anywhere in scope:
    - night light, per-app volume and keep-awake in quick settings;
    - an input-method candidate-window test (SE15 tests only layout switching);
    - per-output fractional scale and HiDPI cursor tests (SE12);
    - a multi-user and kiosk test of the SH10 default preset (open doubt 9);
    - lid-close behaviour;
    - printers (excluded in SE24, although doc_software lists printing as a feature).
- Standard: doc_shell_standard ST3 ("union of every reference"); ISO 29148 (complete).
- Recommendation:
  - Add `display`, `input` and `portal` surfaces to the register (with SPD-08).
  - Move brightness and the power profile out of the battery module into quick settings.
  - Reverse the printers exclusion, at least as a link to the GNOME printers panel.
  - Specify the lid action in Settings' power page.
- Needs a decision: no

### SPD-25 Polish

- Severity: low
- Category: other
- Where: A/doc_shell_standard.md ST4; A/doc_first_run.md:139 (FR5) against A/doc_settings.md:92, :231 (SE22) and docs/decisions/0058-network-privacy-remainder.md; A/doc_settings.md SE7 (:83-97) against SE21 (:218); A/shell-features.md F-cc-37 against forge/config/packages.json:111; A/doc_visual_language.md VL8
- Evidence:
  - ST4 calls the i7-8550U (2017) "five to six years old"; in 2026 it is about nine.
  - The hostname rule differs: first run keeps a generic static hostname "so that no person's name is announced … through DHCP and mDNS", while Settings derives it from the pretty name. 0058 already disables sending the hostname over DHCP.
  - SE7's polkit table lacks the usbguard row that SE21's "Allow always" needs.
  - F-cc-37 says tuned-ppd; the image lists power-profiles-daemon.
  - VL8's list of mark placements omits the bar shield, which 0061 keeps always visible.
- Standard: one source of truth per fact; ISO 29148 (correct).
- Recommendation:
  - Fix the age.
  - State the hostname rule once, in SE22: a generic static name, a personal pretty name, citing 0058.
  - Add the usbguard row with `auth_admin_keep`.
  - Name the power daemon that ships.
  - Align VL8 with 0061.
- Needs a decision: no
