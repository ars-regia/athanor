---
id: SESSION-DAEMONS
title: "doc_session_daemons decisions"
date: 2026-10-05
status: accepted
issues: []
areas: [shell, session, security]
---

# 0015. doc_session_daemons decisions

## Context

Recorded in the maintainer decision log, section 'doc_session_daemons decisions (maintainer, 2026-10-05) — all as recommended', with the controller rulings on conflicts.

## Decision

- 1 four processes: athanor-broker, athanor-idle, athanor-wallpaper, athanor-sessiond
- 2 broker confines apps with systemd unit properties (private XDG_RUNTIME_DIR with allowlist, xdg-dbus-proxy in its own scope, no X11)
- 3 any peer may inhibit idle, each inhibitor logged by app id
- 4 broker runs XDG autostart entries, xdg-desktop-autostart.target dropped
- 5 USBGuard removeRule keeps upstream auth_admin
- 6 allowed-USB list in Settings, Privacy and Security, "USB devices"
- 7 own schemas org.athanor.desktop.idle and org.athanor.desktop.background
- 8 automount while locked: queued, mounted at unlock
- 9 the broker alone launches applications (BR2 launch code moves into it)

Controller rulings on session-daemons conflicts (owed amendments, applied to the drafts in the closing pass):

- PT11: an app's Suspend inhibit flag takes SD5's suspend-only path (portal decision 5); it no longer stops the blank.
- AX5: "no unit masks XDG_RUNTIME_DIR" is limited to shell units; confined application units do mask it.
- OD19: the headset dialog applies through SD15's audio-model command, not the varlink service that leaves with cosmic-settings-daemon.
- Mono audio (mono_sound): owned by doc_accessibility (new AX amendment), served by athanor-sessiond.
- LP5 default timers ship in org.athanor.desktop.idle (SD). Pre-existing defect to report: no CosmicIdle default is shipped today, so the desktop never blanks or suspends.
- LP's mentions of cosmic-idle as caller name athanor-idle after SD step 1.
- FM14's declared limit, SE's missing USBGuard section and SE's startup-apps exclusion are ended by SD11, SD13 and SD10: those drafts cite SD.
- FileManager1 D-Bus activation (FM16): spike S7 in SD. Portal OpenURI through the broker: spike S8, owner doc_portal.
- Control center dark-mode tile writing the COSMIC theme: VL4's existing amendment covers it.

## Consequences

Elaborated in `docs/architecture/doc_session_daemons.md`.
