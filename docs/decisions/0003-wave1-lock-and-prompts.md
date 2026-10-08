---
id: W1-LOCK
title: "doc_lock_and_prompts decisions (wave 1)"
date: 2026-10-05
status: amended by A2-1, A2-6, A2-7
issues: []
areas: [security, shell]
---

# 0003. doc_lock_and_prompts decisions (wave 1)

## Context

Recorded in the maintainer decision log, section 'Wave 1 maintainer decisions', under the heading doc_lock_and_prompts. The options are lettered as in the specification's own question list; the specification lists the alternatives.

## Decision

- D1 A athanor-unlockd@ (maintainer delegated: "ragiona a lungo termine e scegli"); reasons: D3 faillock needs a root writer, polkit 127 took the same model, setuid helpers are being retired, lock stays Landlocked
- D2 A move to athanor-auth, after the lock is proven
- D3 B with-faillock system-wide, deny=10 unlock_time=300
- D4 B count only by default (amends NC5 default); no media controls
- D5 A honour logind Unlock
- D6 B GNOME idle values 5/5, suspend 15 battery, never on mains
- D7 A agent leaves with athanor-osd (joint with OSD M4)
- D8 C fingerprint opt-in, lock AND polkit (maintainer chose against rec)
- D9 A polkit 127 in the forge until F44
- D10 B own cosmic-comp patch only, no upstream proposal (amends SH2)
- D11 B own SystemPrompter now, incl. gcr secret exchange (crypto: implementation plan needs maintainer approval)
- D12 A OSD spec owns end-of-session dialogs

## Consequences

Elaborated in `docs/architecture/doc_lock_and_prompts.md`.

Amended 2026-10-08 by the maintainer (ADR-0090): D3's lockout is `with-faillock` with `deny = 5`, `fail_interval = 900`, `unlock_time = 600`, enabled with the image's PAM change instead of the lock screen's switch.
