---
id: ADR-0080
title: "Pipeline architecture"
date: 2026-10-07
status: amended by ADR-0088, ADR-0098, ADR-0104
issues: []
areas: [build, signing, security]
---

# 0080. Pipeline architecture

## Context

The pipeline review of 2026-10-07 (five audits: architecture, security, performance,
maintainability, benchmark, plus a CRA review) found 24 workflows with overlapping roles,
four rules for `:latest`, a package graph split into levels that no build needs, keys
available to jobs that also build, and no measurable level of supply-chain assurance for
most artifacts. The maintainer asked for a design that holds for the long term, follows
recognised standards, and can be maintained by agents with little effort.

The maintainer also decided, before this record: two signing environments (D43), no
compiler cache for the kernel (PR #250), and update delivery as the first product
priority.

## Decision

1. **Target architecture.** The pipeline converges on `docs/architecture/doc_pipeline.md`:
   six entry workflows (`pr.yml`, `release.yml`, `accept.yml`, `promote.yml`, `bots.yml`,
   `maintenance.yml`), six reusable stages, file contracts between stages, one required
   check (`gate`) fed by change detection, and the plan blocks PB0-PB12 in that order.
2. **Assurance level.** Every artifact a machine or a stage consumes reaches SLSA v1.0
   Build L3, with in-toto attestations, a VSA from acceptance, and a promotion gate that
   evaluates a policy in a repository script. No Kubernetes-based system (Konflux,
   Tekton), no full TUF repository, no second build language.
3. **Two signing environments (D43).** `signing-kernel` holds the Secure Boot and module
   keys; its job signs the NVIDIA modules and `vmlinuz`, published as `azoth-boot`.
   `signing-images` holds the cosign key. Both deploy only from the protected branches
   `iso-v0` and `main`. A job that holds a key builds nothing and runs no third-party
   action, and `verify.py workflows` enforces it. `MOK_PRIVATE_KEY` is retired.
4. **At most two approvals per release cycle:** `signing-kernel` only when the kernel or
   the NVIDIA modules change, `signing-images` always. This amends A2-27, whose two
   approvals per cycle become a ceiling.
5. **Agent identity.** Agents and bots push with GitHub App identities instead of personal
   tokens; once they do, `prevent_self_review` is switched on in both environments.
6. **One package matrix.** The DAG levels are removed; specs build with `--nodeps`, so every
   dirty package builds in one matrix, and the package graph is checked for cycles with
   `graphlib`.

## Consequences

- A2-27 is amended: the ceiling of two approvals replaces "two approvals per release cycle
  stay". Its MOK enrolment part and A2-35 are unchanged.
- `docs/architecture/doc_ci.md` describes the current workflows and points to
  `doc_pipeline.md` as the target until PB12 makes them agree.
- The repository settings gain rulesets, the merge queue, SHA pinning and the two
  environments; each is a maintainer action listed in the plan.
- doc_update_delivery.md (UD6, decision 3 of section 15) and doc_update_trust.md (UT13)
  are amended: the security class is set when the release is dispatched and signed in
  `signing-images` with the images, so promotion holds no key in any class (PQ4).
- Applied by `docs/architecture/doc_pipeline.md`.
