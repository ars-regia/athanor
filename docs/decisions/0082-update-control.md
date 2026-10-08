---
id: ADR-0082
title: "Update control: postpone and opt-out"
date: 2026-10-07
status: amended by ADR-0094
issues: []
areas: [update, security]
---

# 0082. Update control: postpone and opt-out

## Context

A2-5 applies security-class updates at the next shutdown or reboot the user starts, and
A2-26 removes any "Later" for them. CRA Annex I, Part I(2)(c) asks for automatic security
updates enabled by default, together with a clear and easy way to opt out, notification of
available updates, and the option to postpone them temporarily. Without a postpone and an
opt-out, the update policy does not meet that requirement.

## Decision

1. **Automatic by default.** Security-class updates keep A2-5's behaviour: they apply at
   the next shutdown or reboot the user starts, with a notice and the one-step way back.
2. **Time-limited postpone.** The notice offers a postpone, limited in time; when it
   expires, the update applies at the next shutdown. The length is set in the update
   specification.
3. **Opt-out with a warning.** Settings offers a switch that turns automatic application
   off. It needs administrator authorisation, shows a warning before it takes effect, and
   leaves a persistent warning in the trust shield while it is off. Update checks and
   notifications continue while it is off.
4. Feature updates keep their confirmation (UT6).

## Consequences

- A2-26 is amended: "no Later" becomes the bounded postpone above. Its greenboot and
  security-class attestation parts are unchanged.
- A2-5 is amended by the postpone: the next shutdown remains the default, and a postponed
  update applies at the first shutdown after the postpone ends.
- `docs/architecture/doc_update_trust.md` (UT13) and the Settings specification take the
  postpone and the switch; `athanor-update` implements them.
- Applied by `docs/architecture/doc_pipeline.md`.
