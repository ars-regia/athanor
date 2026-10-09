---
id: ADR-0088
title: "Pipeline revision 2"
date: 2026-10-07
status: amended by ADR-0098
issues: []
areas: [build, signing, security]
---

# 0088. Pipeline revision 2

## Context

ADR-0080 accepted the pipeline architecture of `docs/architecture/doc_pipeline.md` at
revision 1. An adversarial review of that architecture on 2026-10-07 (4 blockers, 18 majors,
16 minors) found decisions that do not hold as written: the retirement of `MOK_PRIVATE_KEY`
cannot happen before the image key rotation ends, `prevent_self_review` leaves the releases
the maintainer triggers without a possible approver while the maintainer is the only
reviewer, the self-hosted kernel build cannot claim SLSA Build L3, and releases run on push,
so nothing is dispatched at which the security class could be set. Revision 2 of
`doc_pipeline.md` applies the review and was rewritten on `iso-v0` after PR #265, #266 and
#268; the maintainer approved it on 2026-10-07 and delegated the review of its final text.

PR #265 recorded, without changing a decision of ADR-0080, that the signing jobs of its
decision 3 are jobs of the entry workflow: today `nvidia-kmod-sign` and `sign-system-images`
of `athanor-forge-orchestrator.yml`, in the target the `sign-kernel` and `sign-images` jobs
of `release.yml`, because a called workflow's job reads the secrets of its environment only
when its caller inherits every secret
([actions/runner#4453](https://github.com/actions/runner/issues/4453)).

## Decision

The maintainer decided on 2026-10-07 to amend ADR-0080 as follows:

1. **Decision 1 (target architecture).** The plan blocks gain PB5b, the close of the image
   key rotation, after PB5. The reusable stages gain `call-iso.yml`, which builds and signs
   the ISO after the images are signed: six entry workflows and seven stages.
2. **Decision 2 (assurance level).** SLSA Build L3 is claimed for artifacts built on
   GitHub-hosted runners. Artifacts built on the self-hosted runner (the kernel) and the
   signed derivatives (`azoth-boot`, `azoth-nvidia`), whose provenance
   `call-kernel-publish.yml` generates naming the unsigned digest and the signing run, claim
   Build L2. The policy gate is a repository script or Conforma's standalone `ec` CLI,
   decided by a spike before PB5. Amended 2026-10-08 by the maintainer: the SLSA L3
   statement covers provenance and signing; the Fedora RPM inputs are not pinned until the
   RPM lock lands (PLAT-N10).
3. **Decision 3 (two signing environments).** `MOK_PRIVATE_KEY` is retired at the close of
   the image key rotation (PB5b), not with the creation of the two environments.
4. **Decision 5 (agent identity).** `prevent_self_review` is switched on in both signing
   environments when a second human reviewer is listed in them. Until then the maintainer's
   approval of runs the maintainer triggered is an accepted risk, bounded by the compensating
   controls of `doc_pipeline.md` PL5: the deployment branch restriction to the protected
   branches, the integrity ruleset without bypass, the key isolation of PL11 and no
   administrator bypass. Which actor GitHub records for a push made by the merge queue is
   measured in PB2's gate. Agents on the workstation push with a GitHub App identity, not a
   fine-grained token, because a token of the maintainer would make an agent's pull request
   the maintainer's, which passes under the review bypass and voids the code-owner review of
   PL4; the App's private key lives in the maintainer's secret store.
5. **Consequence on the security class (PQ4).** The security class of a release, with its
   advisory ids, is set and signed when the release is signed, in `signing-images` with the
   images, since releases run on push and are not dispatched; promotion holds no key.
6. **Rulesets.** The product branches carry two rulesets: an integrity ruleset (merge queue,
   required `gate`, linear history, no force push, no deletion) without a bypass actor, and a
   review ruleset whose bypass actor is the repository Admin role, held by the maintainer
   alone, in the "for pull requests only" mode. Amended 2026-10-08 by the maintainer
   (PIPE-N06): the merge queue stays out of the integrity ruleset until every required
   context is reported on `merge_group` (doc_pipeline.md, PL2).
7. **SBOM hashes.** SBOMs carry the strongest hash each ecosystem publishes, and every file
   Athanor builds and delivers carries a SHA-512 hash; the missing SHA-512 of upstream
   components that TR-03183-2 asks for is a documented deviation in `docs/compliance/`.

## Consequences

- Decisions 1, 2, 3 and 5 of ADR-0080 and its consequence on the security class are amended
  as above (items 1 to 5 of this record); decisions 4 and 6 of ADR-0080 are unchanged. Items 6
  and 7 are new decisions that ADR-0080 did not take.
- `doc_pipeline.md` revision 2 applies this record (sections 2, 3.2, 4, 12 and 13).
- Release candidates built in the merge queue are not adopted now (`doc_pipeline.md` PL44).
- `.github/settings/rulesets.json` holds one ruleset today (PR #264); PB2 splits it into the
  two of item 6.
