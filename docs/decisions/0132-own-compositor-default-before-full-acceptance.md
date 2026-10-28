---
id: ADR-0132
title: "The own compositor becomes the default session before its full acceptance"
date: 2026-10-28
status: accepted
issues: []
areas: [shell, security, process]
---

# 0132. The own compositor becomes the default session before its full acceptance

**Accepted** by the maintainer with #810.

## Context

[ADR-0122](0122-own-compositor-optional-session-and-desktop-first.md) decision 1 ships the own
compositor as an optional session, and `doc_own_stack.md` decision 10 makes it the default only
when OS19 holds in full, which includes an hour-long hardware run per machine, the magnifier and
the colour filters. The maintainer asked on 2026-10-28 for the own compositor to become the
default session in the least time that stays correct.

## Decision

The own compositor becomes the image's default session before OS19 holds in full, under the
conditions of `doc_own_stack.md` decision 28, which this record points to and does not repeat.
In short:

1. The gaps of `doc_session.md` SN16 that reach a user who signs in without choosing are closed
   first: the lock, the idle blank, the polkit agent, X11 applications and the key bindings.
2. A user who has the magnifier or a colour filter on is started in cosmic-comp's session, which
   stays at the greeter as the second choice until OS19 holds in full and the overview has come.
3. The hardware run is fifteen minutes per machine in place of the hour.
4. Revision 4 of `doc_session.md` is approved in a pull request of its own before the switch's
   code is written, and reverses its section 6, item 1: a session that fails before its target
   is active, the readiness gate included, hands the sign-in to cosmic-comp's session.
5. The overview follows the switch; release 1.0 waits for it.

- Option A, chosen: **the switch under decision 28's conditions.**
- Option B: **the switch on the full acceptance** (ADR-0122 as written). Rejected: it kept the
  own compositor out of daily use for weeks.
- Option C: **the switch with SN16's gaps accepted.** Rejected: it left the default session
  without a lock, an idle blank or a polkit agent.

## Consequences

- ADR-0122 decision 1 and `doc_own_stack.md` decision 10 are amended; release 1.0 still waits
  for the full acceptance, and takes cosmic-comp's session off the greeter.
- The magnifier, the colour filters and the overview are absent from the default session until
  the own compositor has them; their controls and bindings are hidden or unbound there.
- ADR-0123's "cosmic-comp leaves the default session at 1.0" is read as: cosmic-comp's
  session leaves the default at the switch and the greeter at 1.0.
