---
id: ADR-0115
title: "Ship 0.9 within one week and move rows to 1.1"
date: 2026-10-14
status: accepted
issues: [124, 126, 146, 231, 252]
areas: [process, docs, platform, security, shell]
---

# 0115. Ship 0.9 within one week and move rows to 1.1

## Context

[ADR-0103](0103-audit-5-decisions.md) fixed the scope of 1.0 (D1, D5, D6, D9, D13, D14, D19,
D20, D24 among its decisions). [ADR-0105](0105-releases-before-1-0.md) set four releases before
1.0 (0.3, 0.5, 0.7, 0.9), with 0.9 about 2026-11-27, and said that the scope of 1.0 does not
change. [ADR-0112](0112-pace-to-0-9.md) lifted the freeze, kept the four releases and rejected
tagging only 0.3 and 0.9. The first `:stable` promotion on evidence ran on 2026-10-14 (#146),
the exit of 0.3. The maintainer wants 0.9 as soon as possible and chose, by questions answered
on 2026-10-14, to fit it into one week rather than to merge releases or to cut nothing.
[ADR-0078](0078-fedora-release-target.md) sets the Fedora 45 rebase, with Fedora 44 as the
fallback; Fedora 43 reaches its end of life on 2026-12-02.

## Decision

Decided by the maintainer on 2026-10-14, in questions answered that day. Item 5, except row 10, was
proposed by the agent and accepted with the plan.

1. **Release 0.9 ships within one week, about 2026-10-21.** Rows that do not fit move to 1.1.
2. **Only 0.3 and then 0.9.** The release points 0.5 and 0.7 are dropped. 0.3 stays as defined:
   rows 1 to 6 of `release-1.0.md`, `release.yml` and P2, with the first `:stable` promotion
   (#146) as its exit. Each remaining 0.x is a signed tag plus a `:stable` promotion on
   evidence, with the `release` approval. Work that ADR-0105 placed in 0.5 or 0.7 outside the
   rows joins the release that follows it, 0.9, unless this record moves it.
3. **0.9 ships on Fedora 43.** The rebase to Fedora 45, or to Fedora 44 per ADR-0078, happens
   between 0.9 and 1.0. 1.0 never ships on Fedora 43 and stays before 2026-12-02.
4. **Row 12, for 0.9.** The session lock and the polkit agent are upstream components packaged
   in Fedora, confined and checked. Athanor's own lock, polkit agent and SystemPrompter
   (`doc_lock_and_prompts.md` LP18) move to 1.1, with compositor patch 5.
5. **Moved to 1.1:**
   - row 10: LUKS2 on by default (D6). Anaconda on Fedora 43 has no setting that makes
     encryption default-on. The only patch-free route is an encrypted autopart in the
     kickstart, which preselects the disks and stops asking for the disk; the maintainer
     rejected that trade-off. Until 1.1, encryption stays the person's choice in the
     installer, as `doc_platform_experience.md` section 1 says;
   - row 13: cosmic-comp patches 5 and 6;
   - row 14: the keyring prompter exchange; GNOME Keyring keeps its own gcr prompter until
     then;
   - row 16: Tetragon;
   - row 21: SearXNG, whose section of `doc_software.md` is not written;
   - from row 1: the `-nvidia-legacy` evidence (20 boot-matrix runs, #231). D24 keeps that
     variant unpromoted without it.
6. **Row 7 leaves 0.9** (key hierarchy, not-after, revocation; D5) for the window between 0.9
   and 1.0, with the PB5b rotation and its key ceremony. UT2 already says "before 1.0". Its
   mechanism is not written and the approved text defers it to the revision that builds it, so
   an amendment of UT2 that defines the mechanism comes first, for approval.
7. **Stay in 0.9:** rows 8, 9, 11, 15, 17, 18, 19, 20, 22, 23 and 24, and the rest of row 12
   (bar, dock, launcher, screenshot and screen share acceptance, coexistence), no decision
   moving them. Row 15 (Flathub remote, update timer) is being built: the image has flatpak
   but no remote and no timer today (SWa of `doc_software.md` decision 6).
8. **The hardware work of section 4.3 of `release-1.0.md` stays in 0.9**, minus the
   `-nvidia-legacy` evidence. It needs a maintainer session in that week.

### Proposals that the approval of this record decides

The maintainer did not decide these; they are the agent's placements, and approving this
record approves them.

- **S1 (#124) and P4b (#126) come after 0.9, with the rebase.** They sat in 0.5 (ADR-0112
  item 4); S1 gates P4b and the rebase.
- **The image key rotation closes before 1.0.** The approved definition of PB5b needs a key-2
  release on `:stable` and one more release. With 0.9 and 1.0 the only releases left and row 7
  out of 0.9, either 0.9 carries key 2, which row 7's move forbids, or PB5b closes after 1.0.
  The UT2 amendment of item 6 decides.
- **#252, the ISO acceptance under Secure Boot, stays in 0.9.** It is the "to add" of row 8
  (installer path), which stays in 0.9; the front matter lists it for that reason.
- **The pull request that ships the lock and the polkit agent chooses the upstream
  components.** No component is named here.
- **Work outside the rows placed in 0.5 elsewhere joins 0.9** by the rule of item 2 (see the
  consequences).

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
- **An encrypted autopart in the kickstart for row 10.** It makes encryption default but
  preselects the disks and stops asking for the disk; the maintainer rejected it.

## Consequences

- Fewer release points: the update chain carries one build between 0.3 and 1.0 before the
  rebase, not three. A defect that the removed 0.5 and 0.7 would have caught shows at 0.9 or at
  1.0.
- Security rows ship later. Without patch 5 there is no compositor trusted path for the lock
  and the prompts in 1.0 ([ADR-0041](0041-trusted-path-on-cosmic-comp.md)), and the lock and
  the polkit agent are not Athanor's own. Until the keyring prompter exchange (row 14), GNOME
  Keyring uses its own gcr prompter. There is no Tetragon in 1.0, no `-nvidia-legacy`
  promotion, and encryption is the person's choice at install, not the default.
- Patch 6 is the keyboard monitor that answers Orca alone (`doc_compositor.md`, AX4): until
  1.1 the screen reader lacks that function, and the accessibility work of 0.9 excludes it.
- Row 9: for 0.9, D7's acceptance is met on the greeter's own conversation code. LP18 step 4
  (moving it into `athanor-auth`) waits for Athanor's lock and moves to 1.1 with step 3.
- Row 19: for 0.9 the acceptance is the rule test over the shipped rules. The lock-aware
  `athanor-usbguard-policy` service of LP18 step 10 depends on Athanor's lock and moves with it.
- The rebase falls between 0.9 and 1.0, so the 1.0 image is a different base from the one the
  0.9 hardware session judged. Between the two, the checks that rebase moves (kernel, package
  set, compositor) run again, and 1.0 is judged on the rebased build; if the Fedora 45 image
  is not green by mid-November, the base moves to Fedora 44 (ADR-0078).
- Work that other approved text places in "0.5" outside ADR-0105 joins 0.9 by item 2; none of
  it is moved by this record, and those documents are not edited:
  - ADR-0106 item 7 ("Out of 0.3, into 0.5"): the hourly promotion schedule, upgrade
    acceptance (UD18), the VSA (PL29) and the GitHub Release of PL42;
  - `doc_pipeline.md` line 91: the evidence bundle attached to a GitHub Release "from release
    0.5";
  - `doc_pipeline.md` line 863: the same attachment "from 0.5" in the retention table;
  - `doc_pipeline.md` line 933 (PL42): "From release 0.5, each promotion publishes a GitHub
    Release";
  - `doc_pipeline.md` line 940: "the GitHub Release follows in 0.5 (ADR-0106 item 7)".

  The maintainer's approval of this record accepts that these join 0.9 or asks for them to be
  moved. They read "0.9" where they say "0.5".
- `docs/operations/release-1.0.md` is revision 3: sections 2, 3, 4 and 5 follow this record.
  Row numbers keep their revision 2 value.
- Amended decisions of ADR-0103: D5 (row 7 timing), D6 (row 10), D9 (Tetragon), D13 (lock,
  polkit agent and SystemPrompter), D14 (patches 5 and 6), D19 (SearXNG), D20 (keyring prompter
  exchange) and D24 (the `-nvidia-legacy` evidence). Their texts keep their words; their
  placement in 1.0 changes here, and so do the Tetragon, compositor, lock and prompts
  specifications, which are not edited.
- ADR-0105 is amended in item 1 (the four releases and their contents), item 3 (the
  `-nvidia-legacy` evidence leaves the hardware session) and item 5 ("the scope of 1.0 does not
  change"). ADR-0112 is amended in item 3 (the four releases stay) and item 4 (S1 in 0.5).
  ADR-0106 is amended in item 7 (0.5 as a destination).
