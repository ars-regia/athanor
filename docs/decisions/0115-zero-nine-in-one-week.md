---
id: ADR-0115
title: "Release 0.9 within one week: two releases and five rows moved to 1.1"
date: 2026-10-14
status: accepted
issues: [124, 126, 146, 231, 252]
areas: [process, docs, platform, security, shell]
---

# 0115. Release 0.9 within one week: two releases and five rows moved to 1.1

## Context

[ADR-0105](0105-releases-before-1-0.md) set four releases before 1.0 (0.3, 0.5, 0.7, 0.9),
with 0.9 about 2026-11-27. [ADR-0112](0112-pace-to-0-9.md) lifted the freeze and kept the four
releases, and rejected tagging only 0.3 and 0.9. The first `:stable` promotion on evidence ran
on 2026-10-14 (#146), the exit of 0.3. The maintainer wants 0.9 as soon as possible and chose,
by questions answered on 2026-10-14, to fit it into one week rather than to merge releases or
to cut nothing. [ADR-0078](0078-fedora-release-target.md) sets the Fedora 45 rebase, with
Fedora 44 as the fallback; Fedora 43 reaches its end of life on 2026-12-02.

## Decision

Decided by the maintainer on 2026-10-14. Items 5 and 6 were proposed by the agent and accepted
with the plan.

1. **Release 0.9 ships within one week, about 2026-10-21.** Rows that do not fit move to 1.1.
2. **Only 0.3 and then 0.9.** The release points 0.5 and 0.7 are dropped. 0.3 stays as defined
   (row 3 of `release-1.0.md` and #146). Each remaining 0.x is a signed tag plus a `:stable`
   promotion on evidence, with the `release` approval. Work that ADR-0105 placed in 0.5 or 0.7
   outside the rows follows the release it precedes, except what item 3 places after 0.9.
3. **0.9 ships on Fedora 43.** The rebase to Fedora 45, or to Fedora 44 per ADR-0078, happens
   between 0.9 and 1.0, with spike S1 (#124) and P4b (#126). 1.0 never ships on Fedora 43 and
   stays before 2026-12-02.
4. **Row 12, for 0.9.** The session lock and the polkit agent are upstream components packaged
   in Fedora, confined and checked, chosen in the pull request that ships them. Athanor's own
   lock, polkit agent and SystemPrompter (`doc_lock_and_prompts.md` LP18) move to 1.1, with
   compositor patch 5.
5. **Moved to 1.1:**
   - row 13: cosmic-comp patches 5 and 6;
   - row 14: the keyring prompter exchange; GNOME Keyring keeps its own gcr prompter until
     then;
   - row 16: Tetragon;
   - row 21: SearXNG, whose section of `doc_software.md` is not written;
   - from row 1: the `-nvidia-legacy` evidence (20 boot-matrix runs, #231). D24 keeps that
     variant unpromoted without it.
6. **Stay in 0.9:** row 7 (with expiry and revocation of image keys); rows 8 to 11, 15, 19,
   20, 22 and the rest of row 12 (bar, dock, launcher, screenshot and screen share
   acceptance, coexistence); rows 17, 18, 23 and 24, which no decision moves.
7. **The hardware work of section 4.3 of `release-1.0.md` stays in 0.9**, minus the
   `-nvidia-legacy` evidence. It needs a maintainer session in that week.

## Alternatives rejected

- **Merge releases (0.3, 0.6, 1.0 or similar).** It keeps the whole 1.0 scope but not the
  week: the same rows still have to be accepted before the first tag after 0.3.
- **Cut nothing and keep 2026-11-27.** The date the maintainer wants to leave.
- **Keep the four releases of ADR-0105 and ADR-0112.** Two more approvals and dwell periods,
  and two more end-to-end proofs of the update chain, which the week cannot pay for. The
  proof ADR-0112 valued is now the 0.3 promotion and the 0.9 promotion.
- **Ship Athanor's own lock and prompter in 0.9.** Security-critical code with its acceptance
  on the dev VM, written and confined in one week; Fedora's packaged components carry the
  function meanwhile.

## Consequences

- Fewer release points: the update chain carries one build between 0.3 and 1.0 before the
  rebase, not three. A defect that the removed 0.5 and 0.7 would have caught shows at 0.9 or at
  1.0.
- Security rows ship later. Without patch 5 there is no compositor trusted path for the lock
  and the prompts in 1.0 ([ADR-0041](0041-trusted-path-on-cosmic-comp.md)), and the lock and
  the polkit agent are not Athanor's own. Until the keyring prompter exchange (row 14), GNOME
  Keyring uses its own gcr prompter. There is no Tetragon in 1.0, and no `-nvidia-legacy`
  promotion.
- The rebase falls between 0.9 and 1.0, so the 1.0 image is a different base from the one the
  0.9 hardware session judged. Between the two, the checks that rebase moves (kernel, package
  set, compositor) run again, and 1.0 is judged on the rebased build; if the Fedora 45 image
  is not green by mid-November, the base moves to Fedora 44 (ADR-0078).
- `docs/operations/release-1.0.md` is revision 3: sections 2, 3, 4 and 5 follow this record.
  Row numbers keep their revision 2 value.
- ADR-0105 (items 1, 3 and 5: the four releases and their contents) and ADR-0112 (item 3: the
  four releases stay, S1 in 0.5) are amended by this record. ADR-0103 D9, D14, D19, D20 and
  the Tetragon, compositor, lock and prompts specifications keep their text; their placement
  in 1.0 changes here.
