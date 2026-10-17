---
id: ADR-0121
title: "Athanor writes its own compositor on Smithay and its own toolkit"
date: 2026-10-17
status: accepted
issues: []
areas: [shell, security, process]
---

# 0121. Athanor writes its own compositor on Smithay and its own toolkit

## Context

On 2026-10-16 the maintainer set the long-term direction: Athanor is the Linux kernel plus a
visible stack of its own, a compositor and a toolkit written in Rust, released publicly and open
to contributions. Two approved items say the opposite:

- `doc_shell.md` SH2: "cosmic-comp stays; the dependency has an exit. We do not write a
  compositor", and a fork "starts from cosmic-comp, never from zero".
- `doc_compositor.md` CO4: a fork "starts from cosmic-comp (or from niri, both on Smithay), never
  from zero".

The patch register of `doc_compositor.md` CO3 already carries or plans security and accessibility
features that only a compositor can give: the trusted path for credential prompts (row 5,
ADR-0041 as amended by ADR-0092), the keyboard monitor bound to Orca's cgroup (`doc_accessibility.md` AX4), and the
keyboard aids in the input path (AX11). Spike P4 found that cosmic-comp grants its privileged
globals by an unauthenticated engine string, `com.system76.CosmicPanel`.

Release 1.0 is scoped by ADR-0103 (D1, a narrow 1.0; D13, the desktop tiers, with cosmic-idle,
cosmic-bg, cosmic-workspaces and cosmic-settings coexisting until 1.1; D14, cosmic-comp patches 1,
2, 5 and 6 at 1.0) and dated by ADR-0115 and `docs/operations/release-1.0.md`: 1.0 ships on the
next Fedora base, before Fedora 43 reaches its end of life on 2026-12-02.

## Decision

The maintainer took these decisions on 2026-10-17:

1. **Order: the code starts at once.** The vision document (`doc_own_stack.md`), this record and
   the plans of the first two milestones are written now, and the maintainer approved
   `doc_own_stack.md` revision 1 the same day, so that the code starts at once, beside release 0.9.
   The first two milestones, C1 and T1, are the first steps of the real crates, not spikes to
   archive. The toolkit is built in parallel with the compositor, on the reference laptop; on the
   desktop, the compositor's builds run only while the rig of release 0.9 is idle.
2. **The compositor is new, on Smithay.** It is written from Smithay, not forked from cosmic-comp or
   niri, so that its security model is designed in from the start: privileged protocols granted by
   the client's identity, not by a string it supplies; a trusted path for credential prompts;
   capture confined by policy. cosmic-comp and niri remain references for the hardware matrix, read
   and not copied: cosmic-comp is `GPL-3.0-only`, and code taken from it would bind the compositor
   to that licence. A pull request that ports code from any project names its source and licence,
   and that licence must allow `GPL-3.0-or-later`.
3. **Licences.** The compositor is `GPL-3.0-or-later`, like the repository (A2-3). The toolkit is
   `MIT OR Apache-2.0`, like AccessKit and Parley it builds on, so that third-party applications
   of any licence can adopt it.
4. **Location.** Both live in this repository as crates of their own, each declaring its licence in
   its `Cargo.toml`. They move to repositories of their own, with their history, when outside
   contributors need it.
5. **Release 1.0 waits for the own compositor.** 1.0 ships it in place of cosmic-comp, with the
   GTK4 shell on top, once it passes the acceptance that revision 2 of `doc_own_stack.md` writes
   (section 6 of that document names what it must hold). 1.0 is no longer bound to Fedora 43's end
   of life: the rebase to the next Fedora base ships as a release 0.x before 2026-12-02, so that no
   installed system stays on an unsupported base, and 1.0 follows when the compositor passes. The
   alternative weighed, 1.0 on cosmic-comp with the own compositor in 1.1, kept the hardware
   matrix off the path to 1.0 and was not taken.
6. **Smithay is taken at a pinned git revision**, as niri and cosmic-comp take it, added to
   `deny.toml` with its exact URL; each move of the revision is its own pull request.
7. **The toolkit's widget layer is our own**, on Parley and AccessKit, not on Masonry or Xilem.
8. **X11 applications run through xwayland-satellite**, a separate process started on demand, so
   that no X11 code runs inside the compositor.
9. **NVIDIA's driver is tested from the first milestone**, without the maintainer: headless on the
   desktop's NVIDIA render node (rendering, dmabuf, explicit sync) at every change, and nested in
   the session. Console sessions on KMS (outputs plugged and unplugged, suspend and resume) come
   last, with prepared commands, because they take the maintainer's time.

## Consequences

- `doc_shell.md` SH2 and `doc_compositor.md` CO4 hold until the own compositor passes the
  acceptance of decision 5 and replaces cosmic-comp, in release 1.0; each carries a note naming
  this record. `doc_own_stack.md` revision 1 was approved on 2026-10-17 (decision 1, ADR-0074).
- This record amends ADR-0103 D1 and ADR-0115 in the date of 1.0, which follows the own
  compositor's acceptance instead of Fedora 43's end of life, and ADR-0103 D14, whose cosmic-comp
  patches at 1.0 become requirements of the own compositor (`doc_own_stack.md` OS4). It amends
  ADR-0103 D13 in one point: cosmic-idle, cosmic-bg, cosmic-workspaces and cosmic-settings stay in
  1.0 only through the protocols the own compositor provides them, or are replaced before it
  ships (`doc_own_stack.md` OS5). `release-1.0.md` carries a note naming this record until its
  next revision aligns its rows.
- Release 1.0 waits for the own compositor: its hardware matrix, NVIDIA's driver included, is on
  the path to 1.0. A release 0.x before 2026-12-02 carries the Fedora rebase that ADR-0115 placed
  between 0.9 and 1.0; its number and scope are set in the next revision of `release-1.0.md`. This
  amends ADR-0105, whose sequence of releases before 1.0 gains that release.
- `athanor-compositor-client` (SH2) stays the only crate of the shell that knows the compositor:
  moving to the own compositor rewrites its inside, as SH2 already foresees.
- Patches of the CO3 register that the own compositor makes unnecessary keep their exit condition;
  until the own compositor ships, the register applies unchanged.
- New crates take a `components.toml` entry when they are added, not before.
