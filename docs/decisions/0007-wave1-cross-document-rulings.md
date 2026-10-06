---
id: W1-RULINGS
title: "Cross-document rulings after wave 1"
date: 2026-10-05
status: amended by A2-6, A2-7
issues: []
areas: [shell, security]
---

# 0007. Cross-document rulings after wave 1

## Context

Recorded in the maintainer decision log, section 'Cross-document rulings (controller, 2026-10-05)': rulings that reconcile the wave 1 decisions across specifications.

## Decision

- cosmic-osd retires in one step together with athanor-osd, our polkit agent and the end-of-session dialogs (OSD M4 A, lock D7 A, D12 A). Until then cosmic-osd stays the agent; its broken setuid helper stays reported, not patched; the cosmic-settings-daemon volume defect also waits (OSD M5 B).
- Lock D10 B (own cosmic-comp patch, no upstream proposal) amends doc_shell.md SH2: list it in section 3.
- Lock D8 C: fingerprint also for polkit; login stays password-only.
- Lock D11 B: own SystemPrompter with gcr secret exchange now; the implementation plan of that crypto needs the maintainer's approval before code.
- Lock D1 A: athanor-unlockd@ minimal: user from SO_PEERCRED only, caller must be in the lock's cgroup, PAM service athanor-lock, systemd sandboxing, no network.
- Lock spec must add an acceptance case for cosmic-comp#2702 (keys at the lock reaching an input method holding a grab) (from languages).
- Accessibility: athanor-osk and the input method compete for the single input-method slot per seat (cosmic-osk#44): both drafts state it and AX/LN spikes cover it.
- Accessibility 7: whole-bus AT-SPI proxy with a single replaceable reader gate (SO_PEERCRED + pidfd cgroup); do not build our own Newton; revisit on wayland-protocols !493 or AT-SPI3. Facts in newton-research.md.
- Disks owns the shared udisks model in athanor-services; files and control center use it. Disks 6 noexec default is inherited by the file manager automount.
- doc_visual_language VL9 must list the on-screen display among floating surfaces (amendment to record).
- Super+Space: athanor-shelld owns NextInputSource (LN9); athanor-osd only shows the KeyboardLayout event (controller ruling, OSD row fixed).
- Keyboard-aid notices: os.athanor.Osd1 gains ShowKeyboardAid(s), admitted only from athanor-a11y.service (controller ruling).
- Lock keyboard: the lock embeds the shared athanor-keyboard widget after acceptance item 11 (#2702) passes (controller ruling).

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
