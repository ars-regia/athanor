---
id: PUBLICATION
title: "Publication of the wave 3 specifications"
date: 2026-10-05
status: accepted
issues: []
areas: [docs, process]
---

# 0016. Publication of the wave 3 specifications

## Context

Recorded in the maintainer decision log, section 'Publication (maintainer, 2026-10-05)'. The second entry is a correction by the controller.

## Decision

- One branch and one PR stacked on #119 (visual-language-spec): one commit per specification, plus the amendments to doc_visual_language, doc_bar, doc_launcher, doc_control_center and shell-features in the same PR. After the final consistency review.
- CORRECTION (controller, 2026-10-05): "no CosmicIdle default is shipped, so the desktop never blanks" is NOT established. cosmic-idle 1.8.0 ships no defaults file but has compiled defaults (LP section 1: 15/15/30 min). On the maintainer's desktop ~/.config/cosmic/com.system76.CosmicIdle/v1/* hold None for all three timers, which disables them on that account only. A fresh account is checked by spike L7.

## Consequences

No specification document under `docs/architecture` cites this record yet; it takes effect through the work it describes.
