---
id: ADR-0129
title: "Release 0.9 ships without screenshot and screen share"
date: 2026-10-23
status: accepted
issues: [133, 134, 538]
areas: [process, docs, shell]
---

# 0129. Release 0.9 ships without screenshot and screen share

**Accepted** on 2026-10-23 by the maintainer.

## Context

[ADR-0115](0115-zero-nine-in-one-week.md) item 7 kept in 0.9 the rest of row 12 of
`docs/operations/release-1.0.md`, the desktop session tier, screenshot and screen share
included. The 0.9 inventory of 2026-10-23 found that neither exists in the image: the portal
routing that `xdg-desktop-portal-athanor` installs, `athanor-portals.conf`, answers
`org.freedesktop.impl.portal.Screenshot=none` and `org.freedesktop.impl.portal.ScreenCast=none`,
because no installed backend implements them. `doc_portal.md` builds them in its construction
steps 4 (PT8, screenshots and the colour picker) and 5 (PT9, screen sharing), behind the spikes
S1, S2, S6 and S7, and neither step has started. What is missing is the feature, not only its
acceptance.

## Decision

1. **0.9 ships without screenshot and screen share.** Row 12 stays in 0.9 without them; the
   rest of ADR-0115 item 7 holds.
   - Option A, chosen: **out of 0.9.** The release keeps its date; steps 4 and 5 get the spikes
     they need.
   - Option B: **screenshots only in 0.9** (step 4), screen share later. Rejected: it moves the
     tag by days for one half of the feature.
   - Option C: **both in 0.9.** Rejected: it moves the tag by weeks.
2. **They ship in the first release after steps 4 and 5 of `doc_portal.md` are ready.** Row 12
   keeps them, so 1.0 still requires them; no release before 1.0 waits for them.
3. **The release notes of 0.9 name them as absent.**

## Consequences

- Amended: ADR-0115 item 7, for screenshot and screen share only. Its status line, and its row in
  the index, name this record; its text keeps its words.
- `docs/operations/release-1.0.md` becomes revision 6: the status line and the 0.9 row of
  section 4.1.
- The capture acceptance on the dev VM, which row 12 lists as to add, is written with step 4.
