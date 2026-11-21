---
id: ADR-0147
title: "The keyring prompter is in 1.0, and colour management and HDR come after it"
date: 2026-11-21
status: accepted
issues: [1355]
areas: [security, shell, process, docs]
---

# 0147. The keyring prompter is in 1.0, and colour management and HDR come after it

**Accepted** by the maintainer with the pull request that records it. The maintainer took the two
decisions below on 2026-11-21, with the scope decisions of section 2 of
`docs/superpowers/plans/2026-11-21-road-to-1-0.md`. That plan orders work and approves no text;
this record carries the decisions, and approving it approves them.

## Context

The release of Athanor's own keyring prompter, the `org.gnome.keyring.SystemPrompter` of
`doc_lock_and_prompts.md` LP12, is decided twice, and the two decisions disagree (#1355):

- [ADR-0141](0141-everything-own-in-1-0.md) decision 2 makes the session's prompter Athanor's own
  in 1.0: its session row names "the compositor, greeter (UI and authentication path), lock, polkit
  agent and prompter".
- [ADR-0115](0115-zero-nine-in-one-week.md) decision 5 moves row 14 of `release-1.0.md`, "the
  keyring prompter exchange", to 1.1, and [ADR-0138](0138-own-components-in-1-0.md) decision 1
  keeps it there: "The keyring prompter exchange stays deferred to the Fedora 45 base". ADR-0138's
  Consequences repeat it: "the SystemPrompter stays in 1.1". `release-1.0.md` section 3 follows
  ADR-0138.

Two earlier records bear on the prompter and are not changed here:

- [ADR-0068](0068-session-budget-and-prompter-timing.md) (A2-31) makes the prompter wait for "the
  F45 base with oo7-daemon". The Fedora 45 base landed with #1317. The image still ships
  gnome-keyring, not oo7-daemon (`forge/config/packages.json`, `upstream_core`), and on the
  maintainer's desktop, running the Fedora 45 image on 2026-11-21, `rpm -q` reads
  gnome-keyring 51.1-1.fc45, gcr3 3.41.1-13.fc45 and gcr 4.4.1-1.fc45, with no oo7-daemon.
- [ADR-0085](0085-keyring-prompter-secret-exchange-source.md) takes the secret exchange from upstream
  oo7, and [ADR-0103](0103-audit-5-decisions.md) D20 reverses that dependency "for 1.0": "The keyring
  prompter's exchange is carried by Athanor for 1.0". `doc_lock_and_prompts.md` LP12, L12 and D11
  still follow ADR-0085.

Colour management, 10-bit outputs and HDR are already placed after 1.0 by `doc_own_stack.md`
decision 21 ("8-bit outputs at 1.0", OS15), approved text. The weighted census of 2026-11-21 found
them the only gap of the 2026-11-11 protocol list that Sulphur's source does not cover.

## Decision

1. **Athanor's own keyring prompter is in 1.0.** It is the `org.gnome.keyring.SystemPrompter` of
   `doc_lock_and_prompts.md` LP12, in `athanor-polkit-agent`, drawn with Sal. This confirms
   ADR-0141 decision 2. It returns row 14 of `release-1.0.md` to section 2.
   - Its code waits for two things: the maintainer's approval of the cryptography plan of LP12,
     and spike L9 (`doc_lock_and_prompts.md` section 4). LP18 step 6 stays stop-and-ask
     (`AGENTS.md`, "Stop and ask before editing").
   - This record does not choose the keyring daemon of 1.0 or the source of the exchange's
     cryptography. The LP12 plan answers both, against ADR-0085, ADR-0103 D20 and A2-31.
   - Until the prompter ships, gcr3's `gcr-prompter` draws the keyring prompts, as today.
2. **Colour management, 10-bit outputs and HDR come after 1.0.** Outputs stay at 8 bits in 1.0, as
   `doc_own_stack.md` decision 21 says. Colour gets a revision of its own after 1.0, and no
   release is named for it.

## Alternatives rejected

- **The prompter in 1.1, as ADR-0138 wrote.** The session would ship 1.0 with gcr3's GTK 3
  prompter, a COSMIC-free but non-own program, drawing a password prompt. That contradicts ADR-0141
  decision 2.
- **Colour management in 1.0.** It needs hardware to test it, and Smithay carries none of it
  (`doc_own_stack.md` decision 21 and section 8). It would add to the longest path of 1.0 with no
  requirement of the release behind it.

## Consequences

- `release-1.0.md` revision 10 places row 14 in section 2 and colour management under "Later" in
  section 3.
- `doc_lock_and_prompts.md` LP12, LP18 step 6 and D15 state the 1.0 placement. Its acceptance
  item 12 and the secret exchange's tests of LP17 are no longer deferred to a later milestone.
- The check of #1355, a refusal of the keyring prompter's foreign program, waits for the prompter.
  It cannot refuse the gcr3 RPM, which gnome-keyring 51.1 requires (`rpm -q --whatrequires gcr3`
  on the Fedora 45 image). It refuses instead a second activation of
  `org.gnome.keyring.SystemPrompter`, as spike L9 defines it.
- Amended:
  - [ADR-0138](0138-own-components-in-1-0.md) decision 1: its sentence that the keyring prompter
    exchange stays deferred, and its Consequences' "the SystemPrompter stays in 1.1";
  - [ADR-0115](0115-zero-nine-in-one-week.md) decision 5: its move of row 14 to 1.1.
- The status lines and the index name this record for ADR-0115 and ADR-0138.
