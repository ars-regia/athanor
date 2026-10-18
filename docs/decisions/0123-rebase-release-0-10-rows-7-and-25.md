---
id: ADR-0123
title: "Name the Fedora rebase release 0.10 and place rows 7 and 25 in it"
date: 2026-10-18
status: proposed
issues: [124, 125, 126, 543]
areas: [process, docs, platform, security, update, shell]
---

# 0123. Name the Fedora rebase release 0.10 and place rows 7 and 25 in it

**Proposed.** The maintainer has not taken these decisions. Each item gives the options and the
recommendation; approving this record approves the recommendations, unless the approval names
another option.

## Context

[ADR-0121](0121-own-compositor-and-toolkit.md) decision 5 made release 1.0 wait for Athanor's own
compositor, and took 1.0 off Fedora 43's end of life on 2026-12-02. The rebase to the next Fedora
base, which [ADR-0115](0115-zero-nine-in-one-week.md) placed between 0.9 and 1.0, ships instead as
a release 0.x before that date. ADR-0121 leaves the number and scope of that release to the next
revision of `docs/operations/release-1.0.md`, whose section 5 lets only a decision record change
the release points. [ADR-0078](0078-fedora-release-target.md) keeps Fedora 45 as the target and
Fedora 44 as the fallback if the Fedora 45 image is not green by mid-November 2026; Fedora 44
reaches its end of life on 2027-06-02. [ADR-0122](0122-own-compositor-optional-session-and-desktop-first.md)
ships the own compositor as an optional session before 1.0 and lets 0.9 land some days after
2026-10-21.

Two rows of `release-1.0.md` were placed in 1.0 when 1.0 had a date:

- row 7, the key hierarchy for the image key and the MOK interim key set with not-after and
  revocation (ADR-0103 D5), with the image key rotation PB5b and its key ceremony, in the window
  between 0.9 and 1.0 (ADR-0115 item 6). As `doc_pipeline.md` defines PB5b, its close needs a
  key-2 release on `:stable` and one more release; ADR-0115 left open which release carries key 2;
- row 25, the security-class updates of `doc_update_trust.md` UT13 (#543), placed in 1.0 by
  [ADR-0120](0120-security-class-updates-in-1-0.md). Until it ships, every update waits for the
  person's confirmation and none downloads on a metered connection (ADR-0120 item 3).

With 1.0 undated, both would move with it unless a record places them.

`doc_own_stack.md` revision 1, approved, writes in section 6 step 4 that the own compositor's
acceptance includes the requirements of OS4 tested in its fixture. OS4 turns rows 5 and 6 of the
cosmic-comp patch register (`doc_compositor.md` CO3), the trusted path for credential prompts and
the keyboard monitor that answers Orca alone, into requirements of the own compositor. ADR-0115
placed patches 5 and 6 in 1.1.

## Decision

1. **The rebase release is 0.10 "base".** It ships before 2026-12-02 on Fedora 45, or on Fedora 44
   per ADR-0078, as a signed tag on `iso-v0` plus a `:stable` promotion on evidence (BRN4,
   ADR-0105 item 2).
   - Option A, recommended: **0.10 "base"**. It follows semantic versioning (`v0.10.0` sorts after
     `v0.9.0`), leaves room for a further 0.x release if 1.0 is not out before Fedora 44's end of
     life, and takes the name ADR-0105 gave the release that carried the rebase.
   - Option B: **0.95 "base"**. It reads as "between 0.9 and 1.0" to a person who reads versions as
     decimals, and leaves less obvious room for releases after it.
   - Option C: **0.9.1**. Rejected in this proposal: a change of Fedora base is not a patch
     release, and BRN4 reserves `release/<version>` maintenance for fixes.
2. **Row 7 ships in 0.10, with key 2.**
   - Option A, recommended: **in 0.10**. 0.10 is the key-2 release on `:stable` and 1.0 the release
     after it, so PB5b closes with 1.0. This answers the point ADR-0115 left to the UT2 amendment,
     which still defines the mechanism and needs approval before the code is written.
   - Option B: **in the window before 1.0, unbound to a release.** No installed system gets
     not-after or revocation for the MOK interim key set until a release that has no date.
3. **Row 25 ships in 0.10.** This amends ADR-0120 decision 1 in its release.
   - Option A, recommended: **in 0.10**. While 1.0 waits for the compositor, a security fix
     otherwise reaches a machine only when its person confirms the restart (ADR-0120). The client
     that reads the class must be installed before an update of the security class can apply, so
     the release that carries it is the first after which the class takes effect.
   - Option B: **in 1.0**, as ADR-0120 decided when 1.0 had a date.
4. **The rebase does not wait for rows 7 and 25.** If either is not ready by the Fedora 44
   decision point of ADR-0078 (mid-November 2026), 0.10 ships without it, and a pull request
   naming this record moves the row to the release that follows. This item holds with option A
   of items 2 and 3.
5. **Reading of ADR-0121 decision 5, for confirmation.** The own compositor's acceptance holds
   OS4 in full, as `doc_own_stack.md` section 6 step 4 says: release 1.0 has the compositor's
   trusted path for credential prompts and its keyboard monitor that answers Orca alone. Athanor's
   own session lock, polkit agent and SystemPrompter (`doc_lock_and_prompts.md` LP18) and the
   keyring prompter exchange (row 14) stay in 1.1 (ADR-0115). cosmic-comp patches 5 and 6 are not
   written: cosmic-comp leaves the default session at 1.0.
   - Option A, recommended: **confirm the reading.** It follows approved text, and OS4 calls the
     security model designed in, not patched on.
   - Option B: revision 2 of `doc_own_stack.md` keeps the trusted path and the keyboard monitor out
     of the acceptance, in 1.1, as ADR-0115 placed patches 5 and 6.

## Consequences

- `docs/operations/release-1.0.md` becomes revision 4: section 4.1 gains the row of 0.10, rows 7
  and 25 move to it, and the exit of 1.0 is the own compositor's acceptance (ADR-0121 decision 5).
  Row numbers keep their revision 2 value.
- Amended: ADR-0105, whose releases before 1.0 gain 0.10 (ADR-0121 already amends it there);
  ADR-0115 item 6, in the release of row 7, and its open proposal on PB5b; ADR-0120 decision 1,
  in the release of row 25. Their texts keep their words.
- 0.10 is a different base from the one the 0.9 hardware session judged: the checks the rebase
  moves (kernel, package set, compositor) run again on the 0.10 build, as ADR-0115 says of the
  rebase.
- The 1.0 milestone of the issue tracker carries the work of 1.0; #124, #125, #126 and #543
  move to a 0.10 milestone.
- On approval, this record's status becomes `accepted`, in its front matter and in the index, and
  the status lines of ADR-0105, ADR-0115 and ADR-0120 name it.
