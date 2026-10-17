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
ADR-0041), the keyboard monitor bound to Orca's cgroup (`doc_accessibility.md` AX4), and the
keyboard aids in the input path (AX11). Spike P4 found that cosmic-comp grants its privileged
globals by an unauthenticated engine string, `com.system76.CosmicPanel`.

## Decision

The maintainer took four decisions on 2026-10-17:

1. **Order.** The vision document (`doc_own_stack.md`), this record and the plans of the first two
   spikes are written now, in parallel with release 0.9; they change no shipped file. The spikes,
   which compile and use the rig, start after the 0.9 tag.
2. **The compositor is new, on Smithay.** It is written from Smithay, not forked from cosmic-comp or
   niri, so that its security model is designed in from the start: privileged protocols granted by
   the client's identity, not by a string it supplies; a trusted path for credential prompts;
   capture confined by policy. cosmic-comp and niri remain reference code for the hardware matrix.
3. **Licences.** The compositor is `GPL-3.0-or-later`, like the repository (A2-3). The toolkit is
   `MIT OR Apache-2.0`, like AccessKit and Parley it builds on, so that third-party applications
   of any licence can adopt it; its Apache-2.0-only dependencies, such as Masonry, keep their
   own licence.
4. **Location.** Both live in this repository as crates of their own, each declaring its licence in
   its `Cargo.toml`. They move to repositories of their own, with their history, when outside
   contributors need it.

## Consequences

- `doc_shell.md` SH2 and `doc_compositor.md` CO4 hold until the own compositor replaces
  cosmic-comp under the rule of SH1 (a component leaves only when its replacement passes it); each
  carries a note naming this record. Which release that is, is an open decision of
  `doc_own_stack.md`, which is implemented only once approved (ADR-0074).
- `athanor-compositor-client` (SH2) stays the only crate of the shell that knows the compositor:
  moving to the own compositor rewrites its inside, as SH2 already foresees.
- Patches of the CO3 register that the own compositor makes unnecessary keep their exit condition;
  until the own compositor ships, the register applies unchanged.
- New crates take a `components.toml` entry when they are added, not before.
