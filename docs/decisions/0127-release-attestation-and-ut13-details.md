---
id: ADR-0127
title: "Settle the release attestation and the open details of security-class updates"
date: 2026-10-21
status: accepted
issues: [543]
areas: [update, security, build, release]
---

# 0127. Settle the release attestation and the open details of security-class updates

## Context

Row 25 of `docs/operations/release-1.0.md`, the security-class updates of
`doc_update_trust.md` UT13 (#543), ships in release 0.10
([ADR-0120](0120-security-class-updates-in-1-0.md), as amended by
[ADR-0123](0123-rebase-release-0-10-rows-7-and-25.md)). UT6, UT7 and UT13 at revision 4
([ADR-0094](0094-update-control-settled-points.md), [ADR-0113](0113-update-trust-software-approval.md))
and block PB4b of `doc_pipeline.md` leave points open that the implementation plan of row 25
must cite as fixed values:

- PB4b, the release attestation that carries the class, "is specified when its plan is
  written": it names no predicate type, envelope, transport or signing key. PL55 does not say
  where a `release-class.toml` of ADR-0094 item 4 lives or what it holds, nor which Fedora
  advisories and which updateinfo source the Fedora half reads.
- D43 of `doc_kernel_profile.md`, PL11 of `doc_pipeline.md` and
  [ADR-0080](0080-pipeline-architecture.md) item 3 let no job that holds a key run a
  third-party action. The image signing job signs with `skopeo`; an attestation in the cosign
  format needs `cosign`.
- The opt-out sentences of UT6 and UT13 say that turning "it" on re-locks and turning "it" off
  unlocks, which is right when "it" is the opt-out and backwards when it is the switch
  `enabled`.
- UT7 schema 2 does not say when `postpone` is `null`. UT13 does not say what counts as an
  applied security update or as a boot, nor how the expiry is evaluated "at the shutdown that
  would apply". Acceptance item 19 counts three checks where UT6 holds a digest until a newer
  one is offered.
- UT6 names two refusals of `Postpone()` and not the others. ADR-0094 left open whether a
  change of the switch is logged, while UT6 says it is. ADR-0094 item 2 places the warning texts
  in `doc_update_trust.md`, which has none.
- UT1 counts four units; the package ships six system units and a user unit.

## Decision

The maintainer decided on 2026-10-21, accepting the proposed default of each point, with the
predicate type of point 1 as written below:

1. **Predicate type.** The release attestation has the predicate type
   `https://github.com/ars-regia/athanor/blob/iso-v0/docs/architecture/doc_pipeline.md#pb4b`,
   a URL the project controls that leads to its specification.
2. **Envelope and transport.** An in-toto Statement in a DSSE envelope, attached by
   `cosign attest --key … --type <predicate type> --tlog-upload=false --new-bundle-format=false`
   as `<repo>:sha256-<hex>.att`, with the layer media type
   `application/vnd.dsse.envelope.v1+json`. Retention already keeps `.att` tags (UT10).
   *Amended on 2026-10-21 by the maintainer:* the statement is v0.1, as cosign 3.0.6 writes
   it, or v1, and the client accepts exactly these two `_type` values.
3. **Fields.** The subject is `[{name: <repository>, digest: {sha256: <hex>}}]`. The predicate
   holds `class` (`security` or `feature`), `advisories` (sorted, unique ids), `class_reason`,
   `build_time` (RFC 3339), `revision` (the 40-hex commit), `build_run_id` (a decimal string)
   and `updateinfo_timestamp` (an integer or `null`).
4. **Signing key.** The image key that the policy in force trusts for the repository: key 1
   today, key 2 after PB5b (row 7). The client verifies the attestation with the same
   `keyPaths` as the image signature. It accepts it only when the DSSE signature verifies,
   the predicate type is the one of point 1 and the one subject names the repository and the
   digest it is judging; any mismatch makes the image of the feature class.
5. **D43.** `sign-images` may run `cosign attest` beside the key, with cosign installed by
   `sigstore/cosign-installer` pinned by full commit SHA. `system/sign-images.sh` still starts
   no container. `COSIGN_*` is set only on the steps that sign and attest, never at the job
   level, so the installer step does not receive it.
6. **`release-class.toml`.** One file per motivating change, at
   `release/class/<YYYY-MM-DD>-<slug>.toml`, with `class = "security"`,
   `advisories = ["CVE-…", "FEDORA-…"]` and an optional `repositories = [...]`, whose absence
   means every repository. A file added or modified in `<base>..<candidate>` counts.
7. **The Fedora half of PL55.** Only advisories of `type="security"` count. The updateinfo
   comes from the `updates` repository of the candidate's `VERSION_ID`, reached through the
   Fedora metalink and checked against the `repomd.xml` checksum.
8. **The opt-out wording.** UT6 and the last sentence of UT13's opt-out bullet are read with
   "it" as the opt-out: `SetAutomaticSecurityUpdates(false)` re-locks an unlocked security
   digest, and `SetAutomaticSecurityUpdates(true)` unlocks a waiting one and starts the lineage
   clock if it has not started. Both sentences are rewritten in terms of `enabled`.
9. **When `postpone` is `null`.** While no lineage is open, that is while no security digest
   has reached `will-apply-at-next-shutdown` since the anchor. The anchor is persisted under
   `/var/lib` either way, and `first_notice_at` is never `null` inside the object.
10. **What counts as applied.** The booted digest verifies as the security class, differs from
    the anchor, and `rollbackQueued` is false. A boot that greenboot returns from is not
    applied.
11. **What counts as a boot.** A new `/proc/sys/kernel/random/boot_id` seen by
    `athanor-update check --offline`. A soft reboot keeps the boot id and is not counted.
12. **Item 19's three checks.** UT6's hold stands: the digest stays held until a newer one is
    offered. The acceptance runs three checks and asserts that none downloads it.
13. **The shutdown evaluation.** `athanor-update-state.service` gains `RemainAfterExit=yes`,
    `ExecStop=/usr/bin/athanor-update shutdown`, `After=ostree-finalize-staged.service`, so that
    it stops before that unit, and `TimeoutStopSec=30`. UT1's unit count is corrected.
14. **The refusals of `Postpone()`.** The state is not `will-apply-at-next-shutdown` and nothing
    is postponed: `NothingDownloaded`. The staged digest is of the feature class or is a signed
    copy (UT5): `NotSecurityClass`. The budget is used or expired: `AlreadyPostponed`. The
    opt-out is on: `NothingDownloaded`, because nothing applies by itself, so there is nothing
    to postpone.
15. **Logging of the switch.** Each change is logged to the journal at notice, with the
    fixed `MESSAGE_ID` `d479a5966dcf495db2fa18bf983a72ae` (named
    `os.athanor.update.automatic-changed` in code and tests), the caller's uid and the new value. This
    decides the point ADR-0094 left open.
16. **Texts.** The security notice, its "Postpone" action, the warning shown before the switch
    turns off, the persistent warning row and the `postponed-until <time>` row, in English and
    Italian, are drafted from UT13 and written into UT13 and `doc_bar.md` BR6, with the switch's
    label, the warning's two buttons and the format of `{time}`.
17. **The scope of PB4b for row 25.** Row 25 takes the class derivation and the release
    attestation. The release markers, `mark-current`, the janitor's marker rule and the re-run
    rule stay with the rest of PB4b (ADR-0120). Until markers exist, an unlabelled `:latest` is
    `unlabelled-baseline`.

The switch, its warnings and the "Postpone" button are rows of the existing GTK shield sheet
of BR6; no new surface is made for them.

## Consequences

- [`doc_update_trust.md`](../architecture/doc_update_trust.md): UT1 (point 13 and the unit
  count), UT2 (point 5), UT6 (points 8, 14 and 15), UT7 (point 9), UT11 (point 16) and UT13
  (points 1 to 4, 8, 10 to 13 and 16) are amended.
- [`doc_pipeline.md`](../architecture/doc_pipeline.md): the status, PL11 and PL56 (point 5),
  PL55 (points 6, 7 and 17), section 3.3 (the release statements in `release-candidate`), the
  PB4b row (points 1 to 4 and 17) and section 15.
- [`doc_kernel_profile.md`](../architecture/doc_kernel_profile.md): D43 and section 12 item 1
  (point 5).
- [`doc_bar.md`](../architecture/doc_bar.md): BR6 items 6 and 7 (point 16).
- ADR-0080 is amended in item 3 only: a job that holds a key runs no third-party action except
  the cosign installer of `sign-images`. Its status names this record.
- `scripts/verify.py workflows` admits the pinned cosign installer and the `cosign attest`
  step in `sign-images` only, in the pull request that adds them.
- The code this record specifies still passes the maintainer's checkpoints of `AGENTS.md`
  before it is written: signing, the attestation code, polkit and on-disk state.
