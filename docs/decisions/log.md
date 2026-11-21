# Decision log

One dated line per maintainer decision, newest first, with a link to where the decision is
recorded: a decision record, an issue, a pull request or the document that cites it. A second
maintainer can find a decision here by its date.

The log is an index, not a record. It does not replace a decision record
([README.md](README.md)): a decision that fixes how Athanor is built, shipped, secured or run
still gets one, and the log line then links to it.

A line has the form `YYYY-MM-DD: what was decided. Where.` Documents cite a decision as
"maintainer decision, <date>", and the date of that citation is the date of the line.

- 2026-11-21: the maintainer approved 16 pending specification texts, recorded in their status lines: [doc_files](../architecture/doc_files.md) revision 3, [doc_portal](../architecture/doc_portal.md) revision 3, [doc_threat_model](../architecture/doc_threat_model.md) TM3 for the file chooser, [doc_own_stack](../architecture/doc_own_stack.md) decisions 38 to 40, [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md) D39 to D41, [doc_compositor](../architecture/doc_compositor.md) CO4's note, [doc_launcher](../architecture/doc_launcher.md) LA12, [doc_visual_language](../architecture/doc_visual_language.md) VL1 and VL2, [doc_settings](../architecture/doc_settings.md) revision 3, [doc_accessibility](../architecture/doc_accessibility.md) revision 4 and the AX2 and AX10 amendment, [doc_shell](../architecture/doc_shell.md) SH2's exceptions, [doc_control_center](../architecture/doc_control_center.md) revision 3, [doc_update_delivery](../architecture/doc_update_delivery.md) UD41, [doc_ci](../architecture/doc_ci.md) revision 3, and the citations in [doc_session_daemons](../architecture/doc_session_daemons.md) and doc_control_center. [ADR-0074](0074-agent-and-contributor-model.md).
- 2026-11-21: an NVIDIA pin bump keeps building only the default image before the merge, as an accepted risk, and a test outside the bump's companions holds `lock.py fetch`'s hash check. [doc_build_ordering](../architecture/doc_build_ordering.md) section 4, [#1332](https://github.com/ars-regia/athanor/issues/1332).
- 2026-11-21: no area is exploratory today; an area becomes exploratory only when the maintainer declares it at its start with a dated line in this log, and stops being exploratory when the specification revision written from its spike is approved. [contributing.md](../operations/contributing.md) CT5, [#1290](https://github.com/ars-regia/athanor/issues/1290).
- 2026-11-21: the areas that bring back a full audit are the merge card's full list, and the audit report names the audited range. [audits.md](../operations/audits.md), [contributing.md](../operations/contributing.md) CT6, [#1283](https://github.com/ars-regia/athanor/issues/1283).
- 2026-11-21: a narrowed rule is a rule that allows less; a loosening, or any amendment in the areas listed under "Stop and ask before editing" in `AGENTS.md` or in its inviolable rules, keeps a separate revision. [ADR-0145](0145-small-spec-amendment-in-the-code-pull-request.md), [contributing.md](../operations/contributing.md) CT5, [#1282](https://github.com/ars-regia/athanor/issues/1282).

## Backfill

The lines below are the decisions that `CLAUDE.md`, `.claude/rules/` and the specifications
cite as "maintainer decision, <date>" on 2026-11-21, as far as this first backfill reached; it
does not yet cover every cited date. The `verify.py` check that #1284 owes lists the dates still
missing, and their lines land with it. Each states what was decided and links to
where it is recorded; where one document carries more than about eight decisions of a date, one
line names the document with a short summary, and the document holds the text of each.
A line says only what its source records.

- 2026-11-20: the pre-1.0 audit decisions (the confined class, one build policy, the kernel and alerting gates, the firewall acceptance, the recovery console). [ADR-0143](0143-pre-1-0-audit-decisions.md).
- 2026-11-20: the lock calls `SetLockedHint(true)` when it asks the compositor for the lock, and `SetLockedHint(false)` only after an unlock. [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md) D41.
- 2026-11-19: pull requests of Dependabot are merged as an App pull request is, with a card and an approval. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-11-19: pull requests of the athanor-bots App are followed the same way. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-11-19: the lock and the prompts are built and tested on Sulphur alone; cosmic-comp's session leaves the greeter in 1.0 (ADR-0141 decision 3). [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md) D39.
- 2026-11-19: the lock's secret field refuses an assistive technology's request to set its value. [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md) D40.
- 2026-11-19: the launcher's switch binds Sulphur's session alone, and lands with or after the change that takes cosmic-comp's session out of the greeter. [doc_launcher](../architecture/doc_launcher.md).
- 2026-11-18: a small amendment to an approved specification is approved in the pull request of its code. [ADR-0145](0145-small-spec-amendment-in-the-code-pull-request.md), [#1282](https://github.com/ars-regia/athanor/issues/1282).
- 2026-11-18: after a CLEAN audit, a later head is audited on the delta. [audits.md](../operations/audits.md), [#1283](https://github.com/ars-regia/athanor/issues/1283).
- 2026-11-18: this decision log. [#1284](https://github.com/ars-regia/athanor/issues/1284).
- 2026-11-18: specifications hold contracts only, no crate internals and no line numbers. [contributing.md](../operations/contributing.md) CT5, [#1287](https://github.com/ars-regia/athanor/issues/1287).
- 2026-11-18: in exploratory areas the specification follows the spike. [contributing.md](../operations/contributing.md) CT5, [#1290](https://github.com/ars-regia/athanor/issues/1290).
- 2026-11-18: the file manager and Settings are written on Sal and draw with the toolkit's generated theme, not libadwaita (ADR-0141 decision 5). [doc_visual_language](../architecture/doc_visual_language.md).
- 2026-11-18: the Files specification, revision 3: option (a) of each question. [doc_files](../architecture/doc_files.md).
- 2026-11-18: the Settings specification, revision 3: the first option of each question, and the three changes that follow Sal (no `/dev/dri`, `MemoryDenyWriteExecute=yes` once the window renders with it, `athanor-i18n` in place of `gettext-rs`). [doc_settings](../architecture/doc_settings.md).
- 2026-11-17: the first portal is RemoteDesktop, served through libei sender contexts; InputCapture stays excluded. [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-11-17, also 2026-11-18: touch points never collide; each touch point of an EIS context takes a slot of the seat that no other touch holds. [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-11-17: an injected key reaches only the client, a physical Super released after it opens no launcher, and a key that reaches no client cancels nothing. [doc_portal](../architecture/doc_portal.md).
- 2026-11-17: when a popup that holds the keyboard is destroyed, the keyboard returns at once to its parent popup that still holds a grab, and otherwise to the root surface (#1217, #1231). [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-11-16: a full-screen window's hint is refused only by a layer surface drawn above it; background and bottom layers are ignored. [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-11-16: the wallpaper's decoder admits its peer by control group alone, because its unit's namespaces forbid the read of the peer's executable. [doc_session_daemons](../architecture/doc_session_daemons.md) SD17.
- 2026-11-16: a pull request the maintainer decides will not merge is closed with its branch deleted, or its branch is archived. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-11-15: a merged pull request's worktree is removed, and a cargo target outside it is reported with the command that removes it. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-11-12: a port removes the GTK code in the same pull request. [doc_bar](../architecture/doc_bar.md) (decision 34 of [doc_own_stack](../architecture/doc_own_stack.md)).
- 2026-11-12: CachyOS is a base for performance, never an authority on security: what it changes in the isolation config and code reaches the kernel only after a person has read it, and `prep` checks the files that hold what was read (revision 4). [doc_kernel_build](../architecture/doc_kernel_build.md).
- 2026-11-11: the Smithay bump opens a pull request that is never auto-merged, one at a time while a pull request labelled `smithay-bump` is open. [doc_ci](../architecture/doc_ci.md).
- 2026-11-11: pull requests that change only files under `docs/` stay outside the open pull request limit. [ADR-0139](0139-docs-pull-requests-outside-the-limit.md).
- 2026-11-07: the bar may call `os.athanor.Notifications1` only as the main process of the unit that method admits, checked through systemd and never from `/proc`. [doc_bar](../architecture/doc_bar.md).
- 2026-11-07: the toolkit's layer surfaces, popups and motion are tested under headless sway (`rig.sh toolkit-layer`, `toolkit-popup`, `toolkit-motion`). [doc_ci](../architecture/doc_ci.md).
- 2026-11-07: `athanor-osd` is granted nothing beneath `/proc` and reads no cgroup (as athanor-idle), and OD17 step 1 grants only the top and overlay layers and `ext_workspace_manager_v1`. [doc_osd](../architecture/doc_osd.md).
- 2026-11-07: Sulphur's output management sets sync as well as scale and transform (ADR-0138 decision 4). [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-11-07: a root `Cargo.toml` or `Cargo.lock` that only adds workspace members does not select `shell`. [doc_pipeline](../architecture/doc_pipeline.md).
- 2026-11-07: `athanor-sessiond` runs the appearance reset at each start, and `athanor-appearance-reset.service` leaves the image (SD22 step 3). [doc_session](../architecture/doc_session.md).
- 2026-11-07, several decisions: `athanor-idle` and `athanor-sessiond` are confined with Landlock and write beneath `$XDG_RUNTIME_DIR` as listed (SD17), with amendments to SD1 and a rule for the portal's question that supersedes section 6, decision 13. [doc_session_daemons](../architecture/doc_session_daemons.md).
- 2026-11-06: the own components are placed in 1.0, written in Sal. [ADR-0138](0138-own-components-in-1-0.md).
- 2026-11-06: pull requests land on `iso-v0` through a merge queue. [ADR-0137](0137-merge-queue-on-iso-v0.md).
- 2026-11-06, the brightness slider of a monitor that answers `NotSupported` is hidden, and kept with a note on `Failed`. [doc_control_center](../architecture/doc_control_center.md) (OD4 of [doc_osd](../architecture/doc_osd.md)).
- 2026-11-06, decisions of ADR-0138 for the launcher and the library: both are written in Sal in 1.0, and the launcher is rewritten in Sal. [doc_launcher](../architecture/doc_launcher.md).
- 2026-11-06, decisions D24 to D38 on the lock: the helper is written in Rust (D25), the cost of a socket trigger limit is accepted, and the helper refuses a uid with no active Wayland session (D29). [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md).
- 2026-11-06, decisions on the on-screen display: those of OD4 (the control center is the only caller of the brightness methods, checked by cgroup; the set takes any value from 0 to 100 in one call; the latest value wins; a monitor is identified by make, model and serial; and others) and the placement of the program in 1.0 (ADR-0138). [doc_osd](../architecture/doc_osd.md).
- 2026-11-06, decisions 9 to 14 on the overview and the switcher: the window switcher is `athanor-switcher`, a Sal client of its own and not part of `athanor-overview`, designed in section 8; it uses neither GTK4 nor gtk4-layer-shell (OV20). [doc_overview](../architecture/doc_overview.md).
- 2026-11-06: the inhibitor file of the session has the shape `{"version":1,"inhibitors":[...]}`. [doc_session](../architecture/doc_session.md).
- 2026-11-06, decisions of ADR-0138 on the session daemons: `athanor-idle` is headless on `wayland-client`, `athanor-wallpaper` is a Sal client, the roles of cosmic-settings-daemon are replaced in 1.0, and SD16 carries cosmic-idle's compiled defaults over as 0, never. [doc_session_daemons](../architecture/doc_session_daemons.md).
- 2026-11-05: on a base with no merge queue, the checks of a pull request must have run on the current base before its card. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-11-05: a faster road to 1.0. [ADR-0135](0135-faster-road-to-1-0.md).
- 2026-10-30: the magnifier, the colour filters and the keyboard aids move into the own compositor. [ADR-0133](0133-magnifier-filters-and-keyboard-aids-in-the-own-compositor.md).
- 2026-10-30, decisions of ADR-0133: who shows a latched modifier before `athanor-osd` exists (AX11), and the keys of Athanor's own that cosmic-comp and the own compositor read (AX10). [doc_accessibility](../architecture/doc_accessibility.md).
- 2026-10-30: while sticky keys are on, Super alone latches and shows nothing, and Super then A shows the launcher (AX11). [doc_launcher](../architecture/doc_launcher.md), [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-10-22: a sync pull request of a rebase integration branch is merged with `--merge`. [.claude/rules/workflow.md](../../.claude/rules/workflow.md).
- 2026-10-22: a `pull_request` run with no published kernel skips the build when the merge commit and the base show no difference that shapes it (revision 3). [doc_kernel_build](../architecture/doc_kernel_build.md).
- 2026-10-22: across a base change a release is classified from the git half alone, so only a `security` `release-class.toml` yields `security` (#681). [doc_pipeline](../architecture/doc_pipeline.md).
- 2026-10-19, decisions of ADR-0124 on the visual language, revision 3: it binds only the new toolkit, the toolkit's bar is a 32 px full-width strip, and others. [ADR-0124](0124-visual-language-revision-3.md), [doc_visual_language](../architecture/doc_visual_language.md).
- 2026-10-19: under reduced motion the overlay fades in about 100 ms with no translation, for the fade only. [doc_osd](../architecture/doc_osd.md).
- 2026-10-17: the toolkit draws its own title bar. [doc_own_stack](../architecture/doc_own_stack.md).
- 2026-10-15, #144: the unit fix for the volume keys shipped earlier than the retirement step. [doc_osd](../architecture/doc_osd.md) (four amendments).
- 2026-10-15, #144: the portal's `writable` follows the real access of the user to the file. [doc_portal](../architecture/doc_portal.md).
- 2026-10-14: what waits on the maintainer is reported with the paste-ready command. [.claude/rules/workflow.md](../../.claude/rules/workflow.md). With 2026-10-08 and 2026-10-09, every merge goes through a real approval: [ADR-0103](0103-audit-5-decisions.md) D2, cited in [CLAUDE.md](../../CLAUDE.md).
- 2026-10-14: the greeter's account list and "Other user", decisions D17 to D23. [doc_lock_and_prompts](../architecture/doc_lock_and_prompts.md) LP19, section 6.
- 2026-10-09: low battery belongs to `athanor-sessiond`; one process watches UPower, not two (ADR-0100). [doc_notification_center](../architecture/doc_notification_center.md) NC13.
- 2026-10-09: the audit calendar and the audit types. [ADR-0102](0102-audit-calendar.md).
- 2026-10-09: UD51 stays; it is not part of the Bridge. [doc_update_delivery](../architecture/doc_update_delivery.md).
- 2026-10-08: the at-spi bind is kept into broker-confined applications, a stated leak that ends at the reader gate (AX18 step 6). [ADR-0099](0099-desktop-specs-batch-4.md), [doc_accessibility](../architecture/doc_accessibility.md), [doc_threat_model](../architecture/doc_threat_model.md).
- 2026-10-08: the greeter UI gets no dconf access; it calls one narrow method of `athanor-a11y` that accepts only the five accessibility booleans. [ADR-0099](0099-desktop-specs-batch-4.md), [doc_accessibility](../architecture/doc_accessibility.md).
- 2026-10-08: the mono-audio setting is the boolean `mono` in `org.athanor.desktop.audio`, owned by `athanor-sessiond`. [ADR-0099](0099-desktop-specs-batch-4.md), [doc_accessibility](../architecture/doc_accessibility.md).
- 2026-10-08: the overview's ST5 first complete frame has every card in place with cached thumbnails or icons, and the hot corner is the GNOME key `enable-hot-corners`, off by default in Athanor. [ADR-0099](0099-desktop-specs-batch-4.md), [doc_overview](../architecture/doc_overview.md).
- 2026-10-08: the appearance store is the GNOME keys. [ADR-0099](0099-desktop-specs-batch-4.md), [doc_visual_language](../architecture/doc_visual_language.md).
- 2026-10-08: the trusted path does not wait for the third-party pull request #1441; the patch is carried in the cosmic-comp package (ADR-0041). [doc_compositor](../architecture/doc_compositor.md).
- 2026-10-08: the greeter, `athanor-greeter-session` and the user manager's environment set `GTK_IM_MODULE=simple`. [doc_languages](../architecture/doc_languages.md).
- 2026-10-08: in 1.0 disk encryption is a choice the person makes in the installer, not a default. [doc_platform_experience](../architecture/doc_platform_experience.md), [doc_kernel_profile](../architecture/doc_kernel_profile.md).
- 2026-10-08: remote login is off by default on new installs (ADR-0089). [doc_software](../architecture/doc_software.md).
- 2026-10-06: Athanor does not build its own disk utility; GNOME Disks is the disk utility (A2-14, #159). [doc_disks](../architecture/doc_disks.md).
- 2026-10-06: `athanor-sessiond` and `athanor-a11y-gate` are guards (#156): restarted always, never given up. [doc_session](../architecture/doc_session.md), [doc_session_daemons](../architecture/doc_session_daemons.md).
- 2026-10-06: the seal appears only in the trust shield of the bar (ADR-0077 point 4). [doc_visual_language](../architecture/doc_visual_language.md).
- 2026-10-05: a USB storage device that USBGuard blocks is offered "Allow once / Allow always" in the unlocked session, authorised by polkit; everything stays blocked while locked, which needs the lock to set logind's `LockedHint`. [doc_disks](../architecture/doc_disks.md) decision 10, DK21; [doc_overview](../architecture/doc_overview.md).
- 2026-10-05: removable media and drives mounted by udisks carry `noexec` by default, with no switch in 1.0 (`doc_disks.md` DK20 B). [doc_kernel_profile](../architecture/doc_kernel_profile.md).
- 2026-10-04: Athanor writes its own, real portal backend and an integrated recorder; `doc_portal.md` designs them. [doc_control_center](../architecture/doc_control_center.md) doubt 4, [doc_portal](../architecture/doc_portal.md).
- 2026-09-30: the recovery console takes option A, option B later. [doc_recovery](../architecture/doc_recovery.md).
- 2026-09-30: the software specification, revision 2: every recommendation of section 6. [doc_software](../architecture/doc_software.md) section 6.
- 2026-09-30: a light local AI, and consent to the microphone as a switch in the bar for Athanor's own voice control (AI6). [doc_local_ai](../architecture/doc_local_ai.md).
- 2026-09-25: the end state of the desktop: cosmic-comp is the only COSMIC component left, with GTK4, in a modular way (SH1, SH3). [doc_shell](../architecture/doc_shell.md).
- 2026-09-25: `athanor-sulphur-client` links neither `cosmic-client-toolkit` nor `cosmic-protocols` and stays MIT. [doc_shell](../architecture/doc_shell.md) stage 2a.
- 2026-09-25: the bar specification, revision 1. [doc_bar](../architecture/doc_bar.md).
- 2026-09-19: one crate per program; every surface is its own binary in its own crate. [doc_shell](../architecture/doc_shell.md).
- 2026-09-13: `kernel.yama.ptrace_scope=1` everywhere. [doc_kernel_profile](../architecture/doc_kernel_profile.md).
