---
id: ADR-0128
title: "Move row 7 and image key 2 to 1.0, with a transitional release 0.11"
date: 2026-10-22
status: accepted
issues: []
areas: [process, docs, security, update, build]
---

# 0128. Move row 7 and image key 2 to 1.0, with a transitional release 0.11

**Accepted** on 2026-10-22 by the maintainer.

## Context

[ADR-0123](0123-rebase-release-0-10-rows-7-and-25.md) item 2 placed row 7 of
`docs/operations/release-1.0.md`, the key hierarchy for the image key and the MOK interim key set
with not-after and revocation ([ADR-0103](0103-audit-5-decisions.md) D5), in release 0.10, which
it made the key-2 release on `:stable`. PB5b, the image key rotation, then closes with 1.0. Item 4
of that record moves a row only when it is not ready by the Fedora 44 decision point of
[ADR-0078](0078-fedora-release-target.md), mid-November 2026.

The maintainer decided on 2026-10-22 not to wait for that decision point, because row 7 is not
ready for 0.10:

- `doc_update_trust.md` UT2 does not define the mechanism. The policy of UT3 carries no
  not-after date and no revocation list, so an expired or a revoked key cannot be checked, or
  tested, without guessing where the date and the revocation are read, which clock decides and
  which result they give. UT2 needs an amendment, approved before the code is written.
- The code, the key ceremony and the dev-VM acceptance that follow the amendment are not
  expected to fit before that decision point, confirmed for 2026-11-16 (Fedora 45 rebase plan,
  M5).
- The LUKS2 backup of the keys (ADR-0103 D5, `secrets.md` KC2) is not made yet. It precedes any
  ceremony and any rotation. `secrets.md` section 4.1 step 1 says key 2 was generated into that
  backup; Task A2 of the row 7 plan makes the backup and settles the discrepancy.

[ADR-0103](0103-audit-5-decisions.md) D25 says that no transitional release signed with key 2 is
cut before the key hierarchy of D5 exists. `release-1.0.md` section 5 lets only a decision record
change the order or the release points of the releases before 1.0.

## Decision

1. **Row 7 and key 2 ship in 1.0, together, in a single rotation from key 1 to the key
   hierarchy.** This amends item 2 of ADR-0123, which placed them in 0.10, and item 4 for row 7:
   row 7 no longer waits for the Fedora 44 decision point to move. No release signed
   with key 2 exists before the hierarchy does, as D25 says. "Before 1.0" in ADR-0103 D5 and in
   `doc_update_trust.md` UT2 is read as: the hierarchy exists before 1.0 is signed.
   - Option A, chosen: **in 1.0.** The amendment of UT2, the code and the ceremony get the time
     they need, and the rotation happens once.
   - Option B, the one ADR-0123 had: **key 2 and row 7 in 0.10.** Rejected: it needs the UT2
     amendment, its code and a ceremony before 2026-11-16, on a base the 0.10 rebase also
     changes, and a release that slipped past them would either ship without the row or hold the
     rebase back against item 4 of ADR-0123.
2. **ADR-0103 D25 is kept.** The signing job still does not hold the run, and no transitional
   release is signed with key 2 before the hierarchy exists.
3. **Release 0.10 "base" is signed with key 1.** It drops row 7 and changes nothing else.
4. **A transitional release follows 0.10: 0.11 "anchor", the last 0.x release before 1.0.** It
   is signed with key 1 and carries the new trust anchor in its policy: the public half of the
   hierarchy's root, which machines pin (`doc_update_trust.md` UT2), when Task B1 of the row 7
   plan names a root other than key 2, whose public half every image already trusts. Its scope
   is that and nothing else assigned; fixes that land meanwhile ship in it as usual. It is a new
   0.x row of section 4.1 of `release-1.0.md`. "0.11" names its place, not a fixed number: a
   Fedora rebase needed before 1.0 ships as a release 0.x of its own before it, and the
   transitional release takes the next free number.
5. **1.0 is the first release signed under the hierarchy.** Key 1 is retired, and PB5b closes,
   at the first release after 1.0. This amends the close ADR-0123 item 2 gave PB5b.
6. **The online key lives 180 days and is renewed at 90.**
7. **The step order of the image key rotation.** The switch of step 3, after which `:latest` is
   signed with no approval, merges after 0.11 and before 1.0 is signed.
8. **The LUKS2 key backup is made by the maintainer, outside the repository, before any ceremony.**
   It is not made yet; the plan of row 7 carries it as Task A2.

## Consequences

- Amended: ADR-0123 items 2 and 4; item 4 no longer applies to row 7 and still applies to
  row 25. The rest of
  ADR-0123 holds. Its status line, and its row in the index, name this record; its text keeps its words.
- `docs/operations/release-1.0.md` becomes revision 5: the status line, the counts and lists of
  the releases before 1.0 (sections 1, 4, 4.2 and 5), rows 4 and 7 of section 2, the sentence on
  the LUKS2 backup in section 4, the rows of 0.10, of the new 0.11 and of 1.0 in section 4.1, and
  the row 7 and row 13 entries of section 4.2. The 0.10 row loses row 7 and nothing else.
- The releases before 1.0 are now four: 0.3, 0.9, 0.10 and 0.11. Section 5 of `release-1.0.md`
  names this record beside ADR-0121 and ADR-0123 as one that added a release, and the status
  line of ADR-0105, which set the releases before 1.0, and its row in the index name it too.
- The amendment of UT2 that defines the mechanism of the hierarchy (the not-after date, the
  revocation, the clock and the result) is still to be written, and approved, before the code.
  The plan of row 7, `docs/superpowers/plans/2026-10-22-row-7-key-hierarchy.md`, orders that
  work.
