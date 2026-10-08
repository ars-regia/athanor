---
id: ADR-0094
title: "Update control: the settled points of the postpone and the opt-out"
date: 2026-10-08
status: accepted
issues: []
areas: [update, security]
---

# 0094. Update control: the settled points of the postpone and the opt-out

## Context

ADR-0082 introduced a time-limited postpone and an opt-out for security-class updates but
left their host, authorisation, storage, length and clock rules to the update specification.
`doc_update_trust.md` (UT13) still said that the security class has no "Later", and named a
promotion attestation that PQ4 of `doc_pipeline.md` replaced with the release attestation.
The implementation plan could not cite a specification that contradicts the decision it
implements. The maintainer settled seven points on 2026-10-08, accepting the proposed default
of each, and confirmed the scope of D25.

## Decision

1. **The opt-out switch lives in the trust shield's sheet** (`doc_bar.md`, BR6), where the
   security state already is. Settings keeps no updates page and Software does not host it
   (SW16). A third request on `os.athanor.Update1`, `SetAutomaticSecurityUpdates(b enabled)`,
   is protected by the polkit action `os.athanor.update.set-automatic` with `auth_admin` for
   every subject and no `_keep`. UT6 becomes four requests with the postpone of point 3.
2. **The opt-out is per machine.** It is held in a root-owned file under
   `/var/lib/athanor-update/`, written only by the update service, and published in
   `athanor-trust-state` at schema 2 as `automatic_security_updates: bool`, with every reader
   updated in the same change. The warning texts are written in `doc_update_trust.md`.
3. **The postpone.** An active local session user may postpone without an administrator.
   It is always seven days, counted from the first notice. The staged deployment is re-locked
   and the state reads `postponed-until <time>`. The expiry is stored as wall time and is also
   bounded by seven boots, whichever comes first. A newer security digest replaces the staged
   one and keeps the remaining postpone of the one it replaces; it never extends it. The fourth
   request, `Postpone()`, carries it under the polkit action `os.athanor.update.postpone`, with
   `auth_admin` for any and inactive subjects and `yes` for an active one.
4. **The release attestation is the producer of the class.** A new pipeline block, PB4b,
   produces a key-signed in-toto statement with `build_time`, `class` (`security` or
   `feature`) and `advisories`, signed in `sign-images`. The class comes from a
   `release-class.toml` committed with the motivating change and checked against the PB8
   advisories. PB4b precedes PB10.
5. **An attestation that verifies but has no class field is read as the feature class**, the
   side that asks for a confirmation.
6. **The documents are amended together:** UT6, UT11, UT13 and acceptance items 16 to 18 of
   `doc_update_trust.md`, and BR6 of `doc_bar.md`.
7. **Soft reboot and metered connections.** "The next shutdown or reboot" includes a soft
   reboot the user starts. A security update downloads on a metered connection (NetworkManager
   `Metered` 1 or 3), because CRA Annex I asks for automatic security updates; a feature
   update keeps UT12. With the opt-out on, UT12 holds the download for every class.

D25 of `doc_kernel_profile.md` (blocks that need systemd 262 wait for its final release in
Fedora 45 updates) applies to the 1.1 chain only. The 1.0 update chain uses ostree and GRUB
and does not wait for it.

## Consequences

- ADR-0082 is amended in point 2 (length, count and authorisation of the postpone) and in
  point 3 (the host of the switch is the shield, not Settings). Its other points stand.
- The "no Later" text of A2-26 is replaced by the bounded postpone; the greenboot and
  security-class parts of A2-26 are unchanged.
- `athanor-update` implements the third and fourth requests and `athanor-trust-state`
  schema 2. The two polkit actions need the maintainer's approval before the polkit code is edited.
- The request that carries the postpone is `Postpone()` of UT6 (point 3).
- Whether each change of the switch is logged is not decided by the maintainer; the
  specification does not require it yet.
